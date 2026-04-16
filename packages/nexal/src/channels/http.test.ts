import { afterEach, describe, expect, test } from "bun:test";

import { HttpChannel } from "./http.ts";
import type { IncomingMessage } from "./types.ts";

/**
 * HttpChannel runs a real Bun.serve listener — let each test claim an
 * ephemeral port via the OS and spin a fresh instance. `port: 0` asks
 * the kernel for any free port; `server.port` after start exposes it.
 */

const channels: HttpChannel[] = [];

async function spinUp(onMessage: (m: IncomingMessage) => void): Promise<{
	ch: HttpChannel;
	base: string;
}> {
	const ch = new HttpChannel({ port: 0 });
	channels.push(ch);
	// start() never resolves on its own, so kick it off without await.
	ch.start(onMessage);
	// Wait for the server to report a bound port. Bun.serve is sync in
	// practice but the HttpChannel stores `this.server` inside the
	// fetch closure's parent scope — poll briefly.
	const deadline = Date.now() + 1_000;
	while (Date.now() < deadline) {
		const port = (ch as any).server?.port;
		if (typeof port === "number" && port > 0) {
			return { ch, base: `http://127.0.0.1:${port}` };
		}
		await new Promise((r) => setTimeout(r, 5));
	}
	throw new Error("HttpChannel did not bind in 1s");
}

afterEach(async () => {
	for (const ch of channels.splice(0)) await ch.stop();
});

describe("HttpChannel", () => {
	test("POST /send fires onMessage with parsed chat_id + sender + text", async () => {
		const received: IncomingMessage[] = [];
		const { base } = await spinUp((m) => received.push(m));
		const resp = await fetch(`${base}/send`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ chat_id: "c-1", sender: "alice", text: "hi" }),
		});
		expect(resp.status).toBe(200);
		expect(await resp.json()).toEqual({ ok: true });
		expect(received).toHaveLength(1);
		const m = received[0]!;
		expect(m.channel).toBe("http");
		expect(m.chatId).toBe("c-1");
		expect(m.sender).toBe("alice");
		expect(m.text).toBe("hi");
		expect(m.isMentioned).toBe(true);
	});

	test("POST /send defaults chat_id/sender when body omits them", async () => {
		const received: IncomingMessage[] = [];
		const { base } = await spinUp((m) => received.push(m));
		await fetch(`${base}/send`, {
			method: "POST",
			body: JSON.stringify({ text: "plain" }),
		});
		const m = received[0]!;
		expect(m.chatId).toBe("default");
		expect(m.sender).toBe("http-user");
	});

	test("send() drains through GET /messages, most-recent last, then empty", async () => {
		const { ch, base } = await spinUp(() => undefined);
		await ch.send({ chatId: "c-1", text: "first" });
		await ch.send({ chatId: "c-1", text: "second" });
		// c-2 is a separate outbox and should remain isolated.
		await ch.send({ chatId: "c-2", text: "other chat" });

		const r1 = (await (
			await fetch(`${base}/messages?chat_id=c-1`)
		).json()) as { messages: string[] };
		expect(r1.messages).toEqual(["first", "second"]);

		// Second drain should be empty.
		const r2 = (await (
			await fetch(`${base}/messages?chat_id=c-1`)
		).json()) as { messages: string[] };
		expect(r2.messages).toEqual([]);

		// c-2 still has its item untouched.
		const r3 = (await (
			await fetch(`${base}/messages?chat_id=c-2`)
		).json()) as { messages: string[] };
		expect(r3.messages).toEqual(["other chat"]);
	});

	test("GET /messages without chat_id falls back to 'default' outbox", async () => {
		const { ch, base } = await spinUp(() => undefined);
		await ch.send({ chatId: "default", text: "hello" });
		const j = (await (await fetch(`${base}/messages`)).json()) as {
			messages: string[];
		};
		expect(j.messages).toEqual(["hello"]);
	});

	test("POST /response writes to the outbox (skill-script callback path)", async () => {
		const { base } = await spinUp(() => undefined);
		const resp = await fetch(`${base}/response`, {
			method: "POST",
			body: JSON.stringify({ chat_id: "c-1", text: "from skill" }),
		});
		expect(resp.status).toBe(200);
		const drained = (await (
			await fetch(`${base}/messages?chat_id=c-1`)
		).json()) as { messages: string[] };
		expect(drained.messages).toEqual(["from skill"]);
	});

	test("POST / (root) is also a valid response path", async () => {
		const { base } = await spinUp(() => undefined);
		await fetch(`${base}/`, {
			method: "POST",
			body: JSON.stringify({ chat_id: "c-9", text: "root-post" }),
		});
		const drained = (await (
			await fetch(`${base}/messages?chat_id=c-9`)
		).json()) as { messages: string[] };
		expect(drained.messages).toEqual(["root-post"]);
	});

	test("POST /response silently ignores malformed bodies (no throw)", async () => {
		const { base } = await spinUp(() => undefined);
		const r = await fetch(`${base}/response`, {
			method: "POST",
			body: JSON.stringify({ chat_id: 123 /* wrong type */ }),
		});
		// The server still returns ok: true even when the body wasn't
		// usable — it just doesn't push anything to the outbox.
		expect(r.status).toBe(200);
		const drained = (await (await fetch(`${base}/messages?chat_id=123`)).json()) as {
			messages: string[];
		};
		expect(drained.messages).toEqual([]);
	});

	test("unknown path returns 404", async () => {
		const { base } = await spinUp(() => undefined);
		const resp = await fetch(`${base}/who-am-i`);
		expect(resp.status).toBe(404);
	});

	test("channel name is 'http'", () => {
		const ch = new HttpChannel({ port: 0 });
		expect(ch.name).toBe("http");
	});
});
