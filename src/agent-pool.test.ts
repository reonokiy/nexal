import { describe, expect, mock, test } from "bun:test";

import type { Model } from "@mariozechner/pi-ai";

import { AgentPool } from "./agent-pool.ts";
import type { IncomingMessage } from "./channels/types.ts";
import type { TapeStore } from "./tape/index.ts";
import type { MessageSender } from "./messaging/index.ts";

const mockTapeStore: TapeStore = {
	create: async () => ({ tapeId: "00000000-0000-4000-8000-000000000001" }),
	listTapes: async () => [],
	read: async () => [],
	readPage: async () => [],
	append: async (_tape: any, entryOrEntries: any) =>
		Array.isArray(entryOrEntries)
			? entryOrEntries.map((entry, index) => ({ ...entry, id: index + 1 }))
			: { ...entryOrEntries, id: 1 },
	reset: async () => {},
	delete: async () => {},
	info: async () => ({
		id: "00000000-0000-4000-8000-000000000001",
		entries: 0,
		anchors: 0,
		lastAnchor: null,
		entriesSinceLastAnchor: 0,
		lastTokenUsage: null,
	}),
	handoff: async () => {},
	search: async () => [],
};

const mockSender: MessageSender = {
	send: async () => {},
};

class SpyPool extends AgentPool {
	received: IncomingMessage[] = [];
	constructor() {
		super({
			systemPrompt: "test",
			model: {} as Model<any>,
			tools: [],
			sender: mockSender,
			tapeStore: mockTapeStore,
			getTapeRef: async () => ({ tapeId: "00000000-0000-4000-8000-000000000001" }),
		});
	}
	override handle(msg: IncomingMessage): void {
		this.received.push(msg);
	}
}

describe("AgentPool.forwardChildReport", () => {
	test("splits sessionKey on the first colon and populates channel/chatId", () => {
		const pool = new SpyPool();
		pool.forwardChildReport("telegram:-1001", "worker:refactor", "done");
		expect(pool.received).toHaveLength(1);
		const m = pool.received[0]!;
		expect(m.channel).toBe("telegram");
		expect(m.chatId).toBe("-1001");
	});

	test("sender and text come from the caller", () => {
		const pool = new SpyPool();
		pool.forwardChildReport("http:chat1", "worker:search-agent", "found it");
		const m = pool.received[0]!;
		expect(m.sender).toBe("worker:search-agent");
		expect(m.text).toBe("found it");
	});

	test("synthesized message is marked isMentioned=true", () => {
		const pool = new SpyPool();
		pool.forwardChildReport("telegram:c", "s", "hi");
		expect(pool.received[0]!.isMentioned).toBe(true);
	});

	test("synthesized message has empty metadata and images", () => {
		const pool = new SpyPool();
		pool.forwardChildReport("telegram:c", "s", "hi");
		const m = pool.received[0]!;
		expect(m.metadata).toEqual({});
		expect(m.images).toEqual([]);
	});

	test("timestamp is set to now (monotonic)", () => {
		const pool = new SpyPool();
		const before = Date.now();
		pool.forwardChildReport("telegram:c", "s", "hi");
		const after = Date.now();
		const ts = pool.received[0]!.timestamp;
		expect(ts).toBeGreaterThanOrEqual(before);
		expect(ts).toBeLessThanOrEqual(after);
	});

	test("chatIds with colons in them survive (split on FIRST colon only)", () => {
		// Some chats (e.g. Matrix room ids) embed colons; we split on
		// the first to preserve the rest intact.
		const pool = new SpyPool();
		pool.forwardChildReport("matrix:!room:server.example.com", "s", "hi");
		const m = pool.received[0]!;
		expect(m.channel).toBe("matrix");
		expect(m.chatId).toBe("!room:server.example.com");
	});

	test("malformed sessionKey (no colon) is logged and dropped", () => {
		const pool = new SpyPool();
		pool.forwardChildReport("no-colon-here", "s", "hi");
		// Message should be silently dropped (no crash, nothing queued).
		expect(pool.received).toHaveLength(0);
	});
});
