import { afterEach, describe, expect, test } from "bun:test";

import { WsChannel } from "./ws.ts";
import type { IncomingMessage } from "./types.ts";

/**
 * WsChannel tests — use TCP mode (port: 0 for ephemeral) because
 * Bun's native WebSocket client doesn't support Unix sockets.
 */

const channels: WsChannel[] = [];

async function spinUp(onMessage: (m: IncomingMessage) => void): Promise<{
	ch: WsChannel;
	port: number;
	url: string;
}> {
	const ch = new WsChannel({ port: 0, host: "127.0.0.1" });
	channels.push(ch);
	ch.start(onMessage);

	const deadline = Date.now() + 2_000;
	while (Date.now() < deadline) {
		const srv = (ch as any).server;
		if (srv && typeof srv.port === "number" && srv.port > 0) {
			return {
				ch,
				port: srv.port,
				url: `ws://127.0.0.1:${srv.port}`,
			};
		}
		await new Promise((r) => setTimeout(r, 5));
	}
	throw new Error("WsChannel did not bind in 2s");
}

function openWs(url: string): Promise<WebSocket> {
	return new Promise((resolve, reject) => {
		const ws = new WebSocket(url);
		const timer = setTimeout(() => reject(new Error("WS open timeout")), 2_000);
		ws.addEventListener("open", () => {
			clearTimeout(timer);
			resolve(ws);
		});
		ws.addEventListener("error", () => {
			clearTimeout(timer);
			reject(new Error("WS open error"));
		});
	});
}

function nextMessage(ws: WebSocket): Promise<any> {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => reject(new Error("no WS message in 2s")), 2_000);
		ws.addEventListener(
			"message",
			(ev) => {
				clearTimeout(timer);
				resolve(JSON.parse(typeof ev.data === "string" ? ev.data : ""));
			},
			{ once: true },
		);
	});
}

afterEach(async () => {
	for (const ch of channels.splice(0)) await ch.stop();
	// Give sockets time to close.
	await new Promise((r) => setTimeout(r, 50));
});

describe("WsChannel", () => {
	test("channel name is 'ws'", () => {
		const ch = new WsChannel({ port: 0 });
		expect(ch.name).toBe("ws");
	});

	test("WS client send fires onMessage", async () => {
		const received: IncomingMessage[] = [];
		const { url } = await spinUp((m) => received.push(m));
		const ws = await openWs(url);
		ws.send(JSON.stringify({ type: "send", chat_id: "c1", sender: "alice", text: "hello" }));
		await new Promise((r) => setTimeout(r, 100));
		expect(received).toHaveLength(1);
		const m = received[0]!;
		expect(m.channel).toBe("ws");
		expect(m.chatId).toBe("c1");
		expect(m.sender).toBe("alice");
		expect(m.text).toBe("hello");
		expect(m.isMentioned).toBe(true);
		ws.close();
	});

	test("defaults chat_id and sender when omitted", async () => {
		const received: IncomingMessage[] = [];
		const { url } = await spinUp((m) => received.push(m));
		const ws = await openWs(url);
		ws.send(JSON.stringify({ type: "send", text: "bare" }));
		await new Promise((r) => setTimeout(r, 100));
		expect(received[0]!.chatId).toBe("default");
		expect(received[0]!.sender).toBe("ws-user");
		ws.close();
	});

	test("channel.send() pushes reply to connected client", async () => {
		const { ch, url } = await spinUp(() => undefined);
		const ws = await openWs(url);
		// Register on chat_id "c1" by sending a message first.
		ws.send(JSON.stringify({ type: "send", chat_id: "c1", text: "hi" }));
		await new Promise((r) => setTimeout(r, 50));

		const msgPromise = nextMessage(ws);
		await ch.send({ chatId: "c1", text: "world" });
		const msg = await msgPromise;
		expect(msg).toEqual({ type: "reply", chat_id: "c1", text: "world" });
		ws.close();
	});

	test("replies are isolated by chatId", async () => {
		const { ch, url } = await spinUp(() => undefined);
		const ws1 = await openWs(url);
		const ws2 = await openWs(url);

		ws1.send(JSON.stringify({ type: "send", chat_id: "a", text: "x" }));
		ws2.send(JSON.stringify({ type: "send", chat_id: "b", text: "y" }));
		await new Promise((r) => setTimeout(r, 50));

		const p1 = nextMessage(ws1);
		await ch.send({ chatId: "a", text: "for-a" });
		expect(await p1).toEqual({ type: "reply", chat_id: "a", text: "for-a" });

		// ws2 should NOT have received "for-a" — send to "b" instead.
		const p2 = nextMessage(ws2);
		await ch.send({ chatId: "b", text: "for-b" });
		expect(await p2).toEqual({ type: "reply", chat_id: "b", text: "for-b" });

		ws1.close();
		ws2.close();
	});

	test("POST /send curl fallback fires onMessage", async () => {
		const received: IncomingMessage[] = [];
		const { port } = await spinUp((m) => received.push(m));
		const resp = await fetch(`http://127.0.0.1:${port}/send`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ chat_id: "curl", sender: "bob", text: "via http" }),
		});
		expect(resp.status).toBe(200);
		expect(received).toHaveLength(1);
		expect(received[0]!.chatId).toBe("curl");
		expect(received[0]!.text).toBe("via http");
	});

	test("disconnect removes client from pool", async () => {
		const { ch, url } = await spinUp(() => undefined);
		const ws = await openWs(url);
		ws.send(JSON.stringify({ type: "send", chat_id: "gone", text: "hi" }));
		await new Promise((r) => setTimeout(r, 50));

		ws.close();
		await new Promise((r) => setTimeout(r, 100));

		// send() should silently drop since no clients connected.
		await ch.send({ chatId: "gone", text: "dropped" });
		// No error — just verifying no throw.
	});

	test("malformed WS frames are ignored (no crash)", async () => {
		const received: IncomingMessage[] = [];
		const { url } = await spinUp((m) => received.push(m));
		const ws = await openWs(url);
		ws.send("not json at all");
		ws.send(JSON.stringify({ type: "unknown" }));
		ws.send(JSON.stringify({ type: "send", text: "valid" }));
		await new Promise((r) => setTimeout(r, 100));
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toBe("valid");
		ws.close();
	});
});
