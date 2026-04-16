import { describe, expect, test } from "bun:test";

import {
	type DebounceConfig,
	mergeMessages,
	SessionRunner,
} from "./debounce.ts";
import type { IncomingMessage } from "./types.ts";

function msg(over: Partial<IncomingMessage> = {}): IncomingMessage {
	return {
		channel: "telegram",
		chatId: "-1",
		sender: "alice",
		text: "hi",
		timestamp: Date.now(),
		isMentioned: false,
		metadata: {},
		images: [],
		...over,
	};
}

describe("mergeMessages", () => {
	test("empty input returns null", () => {
		expect(mergeMessages([])).toBeNull();
	});

	test("single message round-trips unchanged", () => {
		const m = msg({ text: "just one" });
		expect(mergeMessages([m])).toBe(m);
	});

	test("multi: text joined by newline, last metadata wins", () => {
		const merged = mergeMessages([
			msg({ text: "one", metadata: { a: 1 }, timestamp: 1 }),
			msg({ text: "two", metadata: { b: 2 }, timestamp: 2 }),
			msg({ text: "three", metadata: { c: 3 }, timestamp: 3 }),
		])!;
		expect(merged.text).toBe("one\ntwo\nthree");
		expect(merged.metadata).toEqual({ c: 3 });
		expect(merged.timestamp).toBe(3);
	});

	test("multi: images from all messages concatenated in order", () => {
		const a = {
			data: new Uint8Array([1]),
			mimeType: "image/png",
			filename: "a.png",
		};
		const b = {
			data: new Uint8Array([2]),
			mimeType: "image/jpeg",
			filename: "b.jpg",
		};
		const merged = mergeMessages([
			msg({ text: "one", images: [a] }),
			msg({ text: "two", images: [] }),
			msg({ text: "three", images: [b] }),
		])!;
		expect(merged.images).toEqual([a, b]);
	});

	test("multi: resulting isMentioned is always true (the batch is hot)", () => {
		const merged = mergeMessages([
			msg({ isMentioned: false }),
			msg({ isMentioned: false }),
		])!;
		expect(merged.isMentioned).toBe(true);
	});
});

describe("SessionRunner", () => {
	const FAST: DebounceConfig = {
		debounceMs: 15,
		delayMs: 80,
		activeWindowMs: 200,
	};

	test("a mentioned message fires once after debounceMs", async () => {
		const received: IncomingMessage[] = [];
		const runner = new SessionRunner("s", FAST, async (m) => {
			received.push(m);
		});
		runner.process(msg({ text: "@bot hi", isMentioned: true }));
		await new Promise((r) => setTimeout(r, 60));
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toBe("@bot hi");
		await runner.shutdown();
	});

	test("rapid mentioned messages collapse into one batch", async () => {
		const received: IncomingMessage[] = [];
		const runner = new SessionRunner("s", FAST, async (m) => {
			received.push(m);
		});
		runner.process(msg({ text: "one", isMentioned: true }));
		runner.process(msg({ text: "two", isMentioned: true }));
		runner.process(msg({ text: "three", isMentioned: true }));
		await new Promise((r) => setTimeout(r, 80));
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toBe("one\ntwo\nthree");
		await runner.shutdown();
	});

	test("unmentioned outside the active window uses a tiny delay", async () => {
		const received: IncomingMessage[] = [];
		const runner = new SessionRunner("s", FAST, async (m) => {
			received.push(m);
		});
		runner.process(msg({ text: "idle chatter", isMentioned: false }));
		// Tiny (~100ms) delay is used when there's no recent mention;
		// wait comfortably beyond that.
		await new Promise((r) => setTimeout(r, 200));
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toBe("idle chatter");
		await runner.shutdown();
	});

	test("unmentioned within active window waits about delayMs", async () => {
		const received: Array<{ text: string; at: number }> = [];
		const runner = new SessionRunner("s", FAST, async (m) => {
			received.push({ text: m.text, at: Date.now() });
		});
		const t0 = Date.now();
		runner.process(msg({ text: "@bot start", isMentioned: true }));
		// Wait enough for the mention dispatch. debounceMs=15 but on
		// some machines the first-tick latency is chunky.
		await new Promise((r) => setTimeout(r, 50));
		expect(received.length).toBeGreaterThanOrEqual(1);

		const sentAt = Date.now();
		runner.process(msg({ text: "follow-up", isMentioned: false }));
		// delayMs=80; give the runner plenty of headroom.
		await new Promise((r) => setTimeout(r, 250));
		expect(received).toHaveLength(2);
		const elapsed = received[1]!.at - sentAt;
		// Within window, we expect delayMs (~80) rather than the tiny
		// UNMENTIONED_DELAY_MS (100). Allow broad slack.
		expect(elapsed).toBeGreaterThanOrEqual(50);
		await runner.shutdown();
	});

	test("shutdown flushes a pending batch before returning", async () => {
		const received: IncomingMessage[] = [];
		const runner = new SessionRunner(
			"s",
			{ debounceMs: 2_000, delayMs: 2_000, activeWindowMs: 2_000 },
			async (m) => {
				received.push(m);
			},
		);
		runner.process(msg({ text: "pending", isMentioned: true }));
		// Shut down before the (2s) timer fires.
		await runner.shutdown();
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toBe("pending");
	});

	test("shutdown without any messages is a no-op (no handler call)", async () => {
		const received: IncomingMessage[] = [];
		const runner = new SessionRunner("s", FAST, async (m) => {
			received.push(m);
		});
		await runner.shutdown();
		expect(received).toHaveLength(0);
	});

	test("handler exceptions are caught so the loop keeps dispatching", async () => {
		const received: IncomingMessage[] = [];
		let first = true;
		const runner = new SessionRunner("s", FAST, async (m) => {
			if (first) {
				first = false;
				throw new Error("kaboom");
			}
			received.push(m);
		});
		const origError = console.error;
		(console as any).error = () => undefined;
		try {
			runner.process(msg({ text: "first", isMentioned: true }));
			await new Promise((r) => setTimeout(r, 100));
			runner.process(msg({ text: "second", isMentioned: true }));
			await new Promise((r) => setTimeout(r, 200));
		} finally {
			(console as any).error = origError;
		}
		// After: the first handler call threw (nothing pushed), the
		// second one succeeded. received should contain only "second".
		expect(received.map((m) => m.text)).toEqual(["second"]);
		await runner.shutdown();
	});
});
