import { afterEach, describe, expect, test } from "bun:test";

import { HeartbeatChannel } from "./heartbeat.ts";
import type { IncomingMessage } from "./types.ts";

const running: HeartbeatChannel[] = [];

afterEach(async () => {
	for (const ch of running.splice(0)) await ch.stop();
});

describe("HeartbeatChannel", () => {
	test("channel name is 'heartbeat'", () => {
		expect(new HeartbeatChannel().name).toBe("heartbeat");
	});

	test("fires the default prompt at intervalMinutes cadence", async () => {
		// 0.001 min = 60ms — fast enough to see multiple ticks.
		const ch = new HeartbeatChannel({ intervalMinutes: 0.001 });
		running.push(ch);
		const received: IncomingMessage[] = [];
		ch.start((m) => received.push(m));
		await new Promise((r) => setTimeout(r, 250));
		expect(received.length).toBeGreaterThanOrEqual(2);
		expect(received[0]!.channel).toBe("heartbeat");
		expect(received[0]!.chatId).toBe("main");
		expect(received[0]!.sender).toBe("system");
		expect(received[0]!.isMentioned).toBe(true);
		expect(received[0]!.text).toMatch(/heartbeat/i);
	});

	test("skips an immediate tick on start — first fire is after interval", async () => {
		const ch = new HeartbeatChannel({ intervalMinutes: 0.01 /* ~600ms */ });
		running.push(ch);
		const received: IncomingMessage[] = [];
		ch.start((m) => received.push(m));
		// Check well before the first interval elapses.
		await new Promise((r) => setTimeout(r, 200));
		expect(received).toHaveLength(0);
	});

	test("custom text override is forwarded verbatim", async () => {
		const ch = new HeartbeatChannel({
			intervalMinutes: 0.001,
			text: "custom nudge",
		});
		running.push(ch);
		const received: IncomingMessage[] = [];
		ch.start((m) => received.push(m));
		await new Promise((r) => setTimeout(r, 150));
		expect(received.length).toBeGreaterThanOrEqual(1);
		expect(received[0]!.text).toBe("custom nudge");
	});

	test("stop() stops emission", async () => {
		const ch = new HeartbeatChannel({ intervalMinutes: 0.001 });
		const received: IncomingMessage[] = [];
		ch.start((m) => received.push(m));
		await new Promise((r) => setTimeout(r, 120));
		const countBefore = received.length;
		expect(countBefore).toBeGreaterThanOrEqual(1);
		await ch.stop();
		await new Promise((r) => setTimeout(r, 200));
		// No new messages after stop.
		expect(received.length).toBe(countBefore);
	});

	test("send() is a no-op for the input-only heartbeat channel", async () => {
		const ch = new HeartbeatChannel();
		await ch.send({ chatId: "whatever", text: "ignored" });
		// No throw — nothing else observable to check.
		expect(true).toBe(true);
	});
});
