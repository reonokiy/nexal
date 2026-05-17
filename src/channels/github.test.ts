import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import { GitHubChannel } from "./github.ts";
import type { IncomingMessage } from "./types.ts";

/**
 * GitHubChannel talks to api.github.com over `fetch`. We stub the global
 * fetch to (a) observe outgoing URL/method/body and (b) feed canned
 * responses in FIFO order. We don't drive the poll loop in `start()`
 * (infinite) — we set `selfLogin` and call the private `tick()` / public
 * `send()` directly through a `(ch as any)` cast, mirroring
 * telegram.test.ts.
 */

interface FakeCall {
	url: string;
	method: string;
	body?: unknown;
	ifModifiedSince?: string;
}

interface CannedResp {
	status?: number;
	ok?: boolean;
	headers?: Record<string, string>;
	jsonData?: unknown;
}

interface FetchMock {
	calls: FakeCall[];
	queue: CannedResp[];
}

const realFetch = globalThis.fetch;
let fm: FetchMock;

beforeEach(() => {
	fm = { calls: [], queue: [] };
	(globalThis as any).fetch = async (url: string, init?: any) => {
		fm.calls.push({
			url,
			method: init?.method ?? "GET",
			body: init?.body ? JSON.parse(String(init.body)) : undefined,
			ifModifiedSince: init?.headers?.["if-modified-since"],
		});
		const n = fm.queue.shift() ?? {};
		const status = n.status ?? 200;
		return {
			status,
			ok: n.ok ?? (status >= 200 && status < 300),
			headers: { get: (k: string) => n.headers?.[k.toLowerCase()] ?? null },
			async json() {
				return n.jsonData ?? {};
			},
		} as any;
	};
});

afterEach(() => {
	(globalThis as any).fetch = realFetch;
});

function newChannel(over?: Partial<GitHubChannelCfg>) {
	const ch = new GitHubChannel({ token: "TEST_PAT", ...over });
	(ch as any).selfLogin = "mybot";
	return ch;
}
type GitHubChannelCfg = ConstructorParameters<typeof GitHubChannel>[0];

function thread(over: any = {}): any {
	return {
		id: "t1",
		unread: true,
		reason: "mention",
		updated_at: "2026-05-17T00:00:00Z",
		url: "https://api.github.com/notifications/threads/t1",
		subject: {
			title: "Fix it",
			url: "https://api.github.com/repos/me/proj/issues/42",
			latest_comment_url: "https://api.github.com/repos/me/proj/issues/comments/9",
			type: "PullRequest",
		},
		repository: { full_name: "me/proj" },
		...over,
	};
}

describe("GitHubChannel.tick", () => {
	test("polls notifications and dispatches a normalized IncomingMessage", async () => {
		fm.queue.push(
			{ headers: { "last-modified": "Sat, 17 May 2026 00:00:00 GMT", "x-poll-interval": "60" }, jsonData: [thread()] },
			{ jsonData: { number: 42, title: "Fix it", body: "body text", html_url: "https://github.com/me/proj/pull/42", state: "open", user: { login: "alice" } } },
			{ jsonData: { body: "please review", html_url: "https://github.com/me/proj/pull/42#c9", user: { login: "alice" } } },
			{ status: 205 },
		);

		const received: IncomingMessage[] = [];
		const ch = newChannel();
		const secs = await (ch as any).tick((m: IncomingMessage) => received.push(m));

		expect(secs).toBe(60);
		expect(received).toHaveLength(1);
		const msg = received[0]!;
		expect(msg.channel).toBe("github");
		expect(msg.chatId).toBe("me/proj#42");
		expect(msg.sender).toBe("alice");
		expect(msg.text).toContain("[github:mention]");
		expect(msg.text).toContain("please review");
		expect(msg.text).toContain("https://github.com/me/proj/pull/42#c9");
		expect(msg.metadata.thread_id).toBe("t1");
		expect(msg.metadata.number).toBe(42);

		const patch = fm.calls.find((c) => c.method === "PATCH");
		expect(patch?.url).toBe("https://api.github.com/notifications/threads/t1");
	});

	test("skips self-authored comment but still marks the thread read", async () => {
		fm.queue.push(
			{ jsonData: [thread()] },
			{ jsonData: { number: 42, title: "Fix it", body: "b", html_url: "h", user: { login: "mybot" } } },
			{ jsonData: { body: "my own reply", html_url: "h#c", user: { login: "mybot" } } },
			{ status: 205 },
		);

		const received: IncomingMessage[] = [];
		const ch = newChannel();
		await (ch as any).tick((m: IncomingMessage) => received.push(m));

		expect(received).toHaveLength(0);
		expect(fm.calls.some((c) => c.method === "PATCH" && c.url.endsWith("/threads/t1"))).toBe(true);
	});

	test("filters out subject types outside the allowlist", async () => {
		fm.queue.push({ jsonData: [thread({ subject: { ...thread().subject, type: "Release" } })] });
		const received: IncomingMessage[] = [];
		const ch = newChannel();
		await (ch as any).tick((m: IncomingMessage) => received.push(m));
		expect(received).toHaveLength(0);
		expect(fm.calls).toHaveLength(1); // only the /notifications GET
	});

	test("304 Not Modified does no work and returns server poll interval", async () => {
		fm.queue.push({ status: 304, ok: false, headers: { "x-poll-interval": "77" } });
		const received: IncomingMessage[] = [];
		const ch = newChannel();
		const secs = await (ch as any).tick((m: IncomingMessage) => received.push(m));
		expect(secs).toBe(77);
		expect(received).toHaveLength(0);
		expect(fm.calls).toHaveLength(1);
	});
});

describe("GitHubChannel.send", () => {
	test("posts a comment to the issue endpoint derived from chatId", async () => {
		fm.queue.push({ status: 201, jsonData: {} });
		const ch = newChannel();
		await ch.send({ chatId: "me/proj#42", text: "hi there" });

		expect(fm.calls).toHaveLength(1);
		expect(fm.calls[0]!.method).toBe("POST");
		expect(fm.calls[0]!.url).toBe("https://api.github.com/repos/me/proj/issues/42/comments");
		expect(fm.calls[0]!.body).toEqual({ body: "hi there" });
	});

	test("ignores a non-issue chatId without calling the API", async () => {
		const ch = newChannel();
		await ch.send({ chatId: "me/proj@t1", text: "noop" });
		expect(fm.calls).toHaveLength(0);
	});
});
