import { afterEach, beforeAll, describe, expect, test } from "bun:test";

import { WsChannel } from "./ws.ts";
import type { IncomingMessage } from "./types.ts";
import {
	createWebSocketConnection,
	type ChatReplyParams,
	type WebSocketConnection,
} from "@nexal/transport";

/**
 * WsChannel tests — use TCP mode (port: 0 for ephemeral) because
 * Bun's native WebSocket client doesn't support Unix sockets.
 *
 * Wire protocol is the @nexal/transport RPC envelope, so tests open a
 * Connection and call `notify(method, params)` instead of sending raw
 * tagged-union frames.
 */

beforeAll(() => {
	// Disable Supabase auth for these tests — keeps the test independent
	// of NEXAL_AUTH_ENABLED in the developer's `.env`.
	process.env.NEXAL_AUTH_ENABLED = "false";
});

const channels: WsChannel[] = [];
const connections: WebSocketConnection[] = [];

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
		const srv = (ch as unknown as { server: { port?: number } | null }).server;
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

async function openConn(url: string): Promise<WebSocketConnection> {
	const conn = await createWebSocketConnection(url);
	connections.push(conn);
	return conn;
}

function nextReply(conn: WebSocketConnection): Promise<ChatReplyParams> {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => reject(new Error("no chat/reply in 2s")), 2_000);
		const off = conn.connection.on("chat/reply", (params) => {
			clearTimeout(timer);
			off();
			resolve(params as ChatReplyParams);
		});
	});
}

afterEach(async () => {
	for (const c of connections.splice(0)) c.connection.close();
	for (const ch of channels.splice(0)) await ch.stop();
	// Give sockets time to close.
	await new Promise((r) => setTimeout(r, 50));
});

describe("WsChannel", () => {
	test("channel name is 'ws'", () => {
		const ch = new WsChannel({ port: 0 });
		expect(ch.name).toBe("ws");
	});

	test("chat/send notification fires onMessage", async () => {
		const received: IncomingMessage[] = [];
		const { url } = await spinUp((m) => received.push(m));
		const conn = await openConn(url);
		conn.connection.notify("chat/send", {
			chatId: "c1",
			sender: "alice",
			text: "hello",
		});
		await new Promise((r) => setTimeout(r, 100));
		expect(received).toHaveLength(1);
		const m = received[0]!;
		expect(m.channel).toBe("ws");
		expect(m.chatId).toBe("c1");
		expect(m.sender).toBe("alice");
		expect(m.text).toBe("hello");
		expect(m.isMentioned).toBe(true);
	});

	test("defaults chatId and sender when omitted", async () => {
		const received: IncomingMessage[] = [];
		const { url } = await spinUp((m) => received.push(m));
		const conn = await openConn(url);
		conn.connection.notify("chat/send", { text: "bare" });
		await new Promise((r) => setTimeout(r, 100));
		expect(received[0]!.chatId).toBe("default");
		expect(received[0]!.sender).toBe("ws-user");
	});

	test("channel.send() pushes chat/reply to connected client", async () => {
		const { ch, url } = await spinUp(() => undefined);
		const conn = await openConn(url);
		// Register on chat_id "c1" by sending a notification first.
		conn.connection.notify("chat/send", { chatId: "c1", text: "hi" });
		await new Promise((r) => setTimeout(r, 50));

		const replyPromise = nextReply(conn);
		await ch.send({ chatId: "c1", text: "world" });
		const params = await replyPromise;
		expect(params).toEqual({ chatId: "c1", text: "world" });
	});

	test("replies are isolated by chatId", async () => {
		const { ch, url } = await spinUp(() => undefined);
		const c1 = await openConn(url);
		const c2 = await openConn(url);

		c1.connection.notify("chat/send", { chatId: "a", text: "x" });
		c2.connection.notify("chat/send", { chatId: "b", text: "y" });
		await new Promise((r) => setTimeout(r, 50));

		const p1 = nextReply(c1);
		await ch.send({ chatId: "a", text: "for-a" });
		expect(await p1).toEqual({ chatId: "a", text: "for-a" });

		// c2 should NOT have received "for-a" — send to "b" instead.
		const p2 = nextReply(c2);
		await ch.send({ chatId: "b", text: "for-b" });
		expect(await p2).toEqual({ chatId: "b", text: "for-b" });
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
		const conn = await openConn(url);
		conn.connection.notify("chat/send", { chatId: "gone", text: "hi" });
		await new Promise((r) => setTimeout(r, 50));

		conn.connection.close();
		await new Promise((r) => setTimeout(r, 100));

		// send() should silently drop since no clients connected.
		await ch.send({ chatId: "gone", text: "dropped" });
		// No error — just verifying no throw.
	});
});
