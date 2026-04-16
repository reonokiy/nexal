import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import { TelegramChannel } from "./telegram.ts";
import type { IncomingMessage } from "./types.ts";

/**
 * TelegramChannel talks to api.telegram.org over `fetch`. These tests
 * stub the global fetch so we can:
 *   - observe the URL + body of outgoing requests
 *   - feed canned Bot API responses into internal flows
 *
 * We don't drive the long-poll loop — the surface area that matters for
 * unit testing is `send()`, `startTyping()`, and the private
 * `handleMessage` / `detectMention` / `extractSender` helpers that
 * shape incoming messages. Those are reached through a `(ch as any)`
 * cast, which is noisy but keeps the test isolated to observable
 * behaviour.
 */

interface FakeFetchCall {
	url: string;
	method: string;
	body?: unknown;
}

interface FetchMock {
	calls: FakeFetchCall[];
	/** Respond in FIFO order; default `{ ok: true, result: ... }`. */
	queue: Array<{ ok: boolean; result?: unknown; description?: string }>;
	/** Raw responses (for binary downloads). */
	rawQueue: Array<{ status: number; bytes: Uint8Array }>;
}

const realFetch = globalThis.fetch;
let fm: FetchMock;

beforeEach(() => {
	fm = { calls: [], queue: [], rawQueue: [] };
	(globalThis as any).fetch = async (url: string, init?: RequestInit) => {
		const call: FakeFetchCall = {
			url,
			method: init?.method ?? "GET",
			body: init?.body ? JSON.parse(String(init.body)) : undefined,
		};
		fm.calls.push(call);

		if (url.includes("/file/bot")) {
			const next = fm.rawQueue.shift() ?? { status: 200, bytes: new Uint8Array([1, 2]) };
			return {
				ok: next.status === 200,
				status: next.status,
				async arrayBuffer() {
					return next.bytes.buffer;
				},
			} as any;
		}

		const next = fm.queue.shift() ?? { ok: true, result: {} };
		return {
			ok: true,
			status: 200,
			async json() {
				return next;
			},
		} as any;
	};
});

afterEach(() => {
	(globalThis as any).fetch = realFetch;
});

function newChannel(over?: Partial<ConstructorParameters<typeof TelegramChannel>[0]>) {
	return new TelegramChannel({
		botToken: "TEST_TOKEN",
		longPollTimeoutSec: 1,
		...over,
	});
}

function msg(over: any = {}): any {
	return {
		message_id: 101,
		from: { id: 999, is_bot: false, username: "alice" },
		chat: { id: -1, type: "group" },
		date: Math.floor(Date.now() / 1000),
		text: "hello",
		...over,
	};
}

describe("TelegramChannel.send()", () => {
	test("builds the correct Bot API URL and body", async () => {
		fm.queue.push({ ok: true, result: {} });
		const ch = newChannel();
		await ch.send({ chatId: "-123", text: "hi there" });
		expect(fm.calls).toHaveLength(1);
		const c = fm.calls[0]!;
		expect(c.url).toBe("https://api.telegram.org/botTEST_TOKEN/sendMessage");
		expect(c.method).toBe("POST");
		expect(c.body).toEqual({ chat_id: "-123", text: "hi there" });
	});

	test("replyTo populates reply_parameters with the numeric message id", async () => {
		fm.queue.push({ ok: true, result: {} });
		const ch = newChannel();
		await ch.send({ chatId: "-1", text: "thx", replyTo: "42" });
		expect(fm.calls[0]!.body).toEqual({
			chat_id: "-1",
			text: "thx",
			reply_parameters: { message_id: 42 },
		});
	});

	test("throws when the Bot API reports ok=false with a description", async () => {
		fm.queue.push({ ok: false, description: "chat not found" });
		const ch = newChannel();
		await expect(ch.send({ chatId: "gone", text: "x" })).rejects.toThrow(
			/sendMessage: chat not found/,
		);
	});
});

describe("TelegramChannel.startTyping()", () => {
	test("calls sendChatAction 'typing' and stops on handle.stop()", async () => {
		// Queue several responses so the polling loop can drain without throwing.
		for (let i = 0; i < 5; i++) fm.queue.push({ ok: true, result: true });
		const ch = newChannel();
		const handle = ch.startTyping("-42")!;
		// First tick is sync inside the loop.
		await new Promise((r) => setTimeout(r, 10));
		const typingCalls = fm.calls.filter((c) => c.url.endsWith("/sendChatAction"));
		expect(typingCalls.length).toBeGreaterThanOrEqual(1);
		expect(typingCalls[0]!.body).toEqual({ chat_id: "-42", action: "typing" });
		handle.stop();
	});
});

describe("TelegramChannel.stop()", () => {
	test("clears buffered media groups and their timers", async () => {
		const ch = newChannel();
		const groups: Map<string, any> = (ch as any).mediaGroups;
		const timer = setTimeout(() => undefined, 5_000);
		groups.set("g1", { items: [], timer });
		expect(groups.size).toBe(1);
		await ch.stop();
		expect(groups.size).toBe(0);
	});
});

describe("private: extractSender via cast", () => {
	// `extractSender` is a module-local helper — we reach it via
	// `handleMessage` and inspect the synthesized IncomingMessage.
	async function runAndGrab(ch: TelegramChannel, m: any): Promise<IncomingMessage | null> {
		let got: IncomingMessage | null = null;
		await (ch as any).handleMessage(m, (im: IncomingMessage) => {
			got = im;
		});
		return got;
	}

	test("uses from.username + from.id by default", async () => {
		const ch = newChannel();
		const im = await runAndGrab(ch, msg({ from: { id: 7, username: "bob" }, text: "hi" }));
		expect(im?.sender).toBe("bob");
		expect(im?.metadata.user_id).toBe("7");
	});

	test("Channel_Bot forward with author_signature → uses the signature", async () => {
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				from: { id: 1, username: "Channel_Bot" },
				author_signature: "Anna",
				text: "hi",
			}),
		);
		expect(im?.sender).toBe("Anna");
	});

	test("GroupAnonymousBot + sender_chat falls through to sender_chat username", async () => {
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				from: { id: 1, username: "GroupAnonymousBot" },
				sender_chat: { id: 99, type: "channel", username: "acme_news" },
				text: "hi",
			}),
		);
		expect(im?.sender).toBe("acme_news");
		expect(im?.metadata.user_id).toBe("99");
	});
});

describe("private: detectMention via handleMessage", () => {
	async function runAndGrab(ch: TelegramChannel, m: any): Promise<IncomingMessage | null> {
		let got: IncomingMessage | null = null;
		await (ch as any).handleMessage(m, (im: IncomingMessage) => {
			got = im;
		});
		return got;
	}

	test("private chat is always mentioned", async () => {
		const ch = newChannel();
		(ch as any).botUsername = "thebot";
		const im = await runAndGrab(
			ch,
			msg({ chat: { id: 1, type: "private" }, text: "hey" }),
		);
		expect(im?.isMentioned).toBe(true);
	});

	test("reply-to-bot is mentioned", async () => {
		const ch = newChannel();
		(ch as any).botUsername = "thebot";
		const im = await runAndGrab(
			ch,
			msg({
				chat: { id: 1, type: "group" },
				reply_to_message: { from: { id: 2, is_bot: true, username: "thebot" } } as any,
				text: "ty",
			}),
		);
		expect(im?.isMentioned).toBe(true);
	});

	test("@mention in text flips isMentioned even for groups", async () => {
		const ch = newChannel();
		(ch as any).botUsername = "thebot";
		const im = await runAndGrab(
			ch,
			msg({ chat: { id: 1, type: "group" }, text: "hi @thebot" }),
		);
		expect(im?.isMentioned).toBe(true);
	});

	test("plain group message without mention is NOT mentioned", async () => {
		const ch = newChannel();
		(ch as any).botUsername = "thebot";
		const im = await runAndGrab(
			ch,
			msg({ chat: { id: 1, type: "group" }, text: "chatting with friends" }),
		);
		expect(im?.isMentioned).toBe(false);
	});
});

describe("private: handleMessage ACL", () => {
	test("disallowed chat + disallowed user → sends 'Not authorized' and drops the message", async () => {
		fm.queue.push({ ok: true, result: {} });
		const ch = newChannel({
			allowChats: ["-999"],
			allowFrom: ["trusted"],
		});
		(ch as any).botUsername = "thebot";
		let seen = 0;
		await (ch as any).handleMessage(
			msg({
				chat: { id: -1, type: "group" },
				from: { id: 2, username: "stranger" },
				text: "hi",
			}),
			() => seen++,
		);
		expect(seen).toBe(0);
		expect(fm.calls).toHaveLength(1);
		expect(fm.calls[0]!.body).toMatchObject({
			chat_id: "-1",
			text: expect.stringContaining("Not authorized"),
		});
	});

	test("allowChats passes the chat through even if user isn't in allowFrom", async () => {
		const ch = newChannel({
			allowChats: ["-1"],
			allowFrom: ["trusted"],
		});
		(ch as any).botUsername = "thebot";
		const received: IncomingMessage[] = [];
		await (ch as any).handleMessage(
			msg({ chat: { id: -1, type: "group" }, from: { id: 2, username: "stranger" }, text: "hi" }),
			(m: IncomingMessage) => {
				received.push(m);
			},
		);
		expect(received).toHaveLength(1);
		expect(received[0]!.sender).toBe("stranger");
	});

	test("Channel_Bot forwards bypass allowFrom filter", async () => {
		const ch = newChannel({ allowFrom: ["owner"] });
		(ch as any).botUsername = "thebot";
		const received: IncomingMessage[] = [];
		await (ch as any).handleMessage(
			msg({
				chat: { id: -1, type: "channel" },
				from: { id: 1, username: "Channel_Bot" },
				author_signature: "Editor",
				text: "post",
			}),
			(m: IncomingMessage) => {
				received.push(m);
			},
		);
		expect(received[0]?.sender).toBe("Editor");
	});
});

describe("private: extractContent (text-only path is exercised above) — non-text variants", () => {
	async function runAndGrab(ch: TelegramChannel, m: any): Promise<IncomingMessage | null> {
		let got: IncomingMessage | null = null;
		await (ch as any).handleMessage(m, (im: IncomingMessage) => {
			got = im;
		});
		return got;
	}

	test("document → caption + summary with file_id + mime", async () => {
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				text: undefined,
				caption: "report",
				document: {
					file_id: "BQAD1234",
					file_name: "q3.pdf",
					mime_type: "application/pdf",
				},
			}),
		);
		expect(im?.text).toContain("report");
		expect(im?.text).toContain("[received document: q3.pdf (application/pdf)");
		expect(im?.text).toContain("BQAD1234");
		expect(im?.images).toEqual([]);
	});

	test("voice → summary text with file_id", async () => {
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				text: undefined,
				voice: { file_id: "VAX123" },
			}),
		);
		expect(im?.text).toContain("[received voice message");
		expect(im?.text).toContain("VAX123");
	});

	test("photo path calls getFile + downloads, populates images", async () => {
		// First queued response → getFile.
		fm.queue.push({ ok: true, result: { file_path: "photos/abc.jpg" } });
		// Second raw response → the downloaded bytes.
		fm.rawQueue.push({ status: 200, bytes: new Uint8Array([10, 20, 30]) });
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				text: undefined,
				caption: "look",
				photo: [
					{ file_id: "small", file_unique_id: "s1", width: 50, height: 50 },
					{ file_id: "big", file_unique_id: "b1", width: 500, height: 500 },
				],
			}),
		);
		expect(im?.text).toBe("look");
		expect(im?.images).toHaveLength(1);
		expect(im?.images[0]!.mimeType).toBe("image/jpeg");
		expect(im?.images[0]!.filename).toBe("b1.jpg");
		const bytes = im?.images[0]!.data as Uint8Array;
		expect(Array.from(bytes)).toEqual([10, 20, 30]);
	});

	test("photo path survives getFile failure — no throw, images stay empty", async () => {
		fm.queue.push({ ok: false, description: "boom" });
		const ch = newChannel();
		const im = await runAndGrab(
			ch,
			msg({
				text: undefined,
				caption: "pic",
				photo: [{ file_id: "f", file_unique_id: "u", width: 1, height: 1 }],
			}),
		);
		expect(im?.text).toBe("pic");
		expect(im?.images).toEqual([]);
	});
});

describe("private: media-group buffering", () => {
	test("items with the same media_group_id are merged into one IncomingMessage", async () => {
		fm.queue.push({ ok: true, result: { file_path: "p1.jpg" } });
		fm.rawQueue.push({ status: 200, bytes: new Uint8Array([1]) });
		fm.queue.push({ ok: true, result: { file_path: "p2.jpg" } });
		fm.rawQueue.push({ status: 200, bytes: new Uint8Array([2]) });

		const ch = newChannel();
		const received: IncomingMessage[] = [];
		const onMessage = (m: IncomingMessage) => received.push(m);

		// Put the buffer timer in fast-mode for the test by shrinking it
		// — not possible from the outside, so just wait 1600ms.
		await (ch as any).handleMessage(
			msg({
				message_id: 1,
				media_group_id: "MG-1",
				text: undefined,
				caption: "caption A",
				photo: [{ file_id: "1", file_unique_id: "u1", width: 1, height: 1 }],
			}),
			onMessage,
		);
		await (ch as any).handleMessage(
			msg({
				message_id: 2,
				media_group_id: "MG-1",
				text: undefined,
				caption: "caption B",
				photo: [{ file_id: "2", file_unique_id: "u2", width: 1, height: 1 }],
			}),
			onMessage,
		);

		// Nothing dispatched yet — still buffered.
		expect(received).toHaveLength(0);

		// Timer is 1500ms — wait for it to fire.
		await new Promise((r) => setTimeout(r, 1700));
		expect(received).toHaveLength(1);
		const m = received[0]!;
		expect(m.metadata.album).toBe(true);
		expect(m.images).toHaveLength(2);
		expect(m.text).toBe("caption A\ncaption B");
	});
});
