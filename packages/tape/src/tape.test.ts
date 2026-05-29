import { describe, expect, test } from "bun:test";

import {
	Tape,
	TapeEvent,
	createAppendThresholdHooks,
	createAutoSummaryHooks,
	createExternalSummaryManager,
	createSummaryContextManager,
	createTailContextManager,
} from "./index.ts";
import type { TapeEntry, TapeEntryDraft, TapeHandle, TapeInfo, TapeStore } from "./index.ts";

describe("Tape system configuration", () => {
	test("setSystemPrompt records prompt metadata and skips unchanged prompts when requested", async () => {
		const { tape, entries } = makeTape();

		const first = await tape.setSystemPrompt("you are helpful", {
			scope: "session",
			metadata: { sessionKey: "telegram:1" },
			ifChanged: true,
		});
		const second = await tape.setSystemPrompt("you are helpful", {
			scope: "session",
			metadata: { sessionKey: "telegram:1" },
			ifChanged: true,
		});

		expect(first?.kind).toBe("event");
		expect(first?.payload.name).toBe(TapeEvent.System.Prompt);
		expect(first?.payload.data).toEqual({
			scope: "session",
			systemPrompt: "you are helpful",
			metadata: { sessionKey: "telegram:1" },
		});
		expect(first?.meta).toEqual({ internal: true, scope: "session", change: "changed" });
		expect(second).toBeNull();
		expect(entries).toHaveLength(1);
		expect(await tape.systemPromptStatus("you are helpful", {
			scope: "session",
			metadata: { sessionKey: "telegram:1" },
		})).toBe("current");
		expect(await tape.systemPromptStatus("new prompt", {
			scope: "session",
			metadata: { sessionKey: "telegram:1" },
		})).toBe("changed");
	});

	test("setModel records model configuration without tools", async () => {
		const { tape } = makeTape();

		const record = await tape.setModel({
			id: "kimi-k2.6",
			name: "kimi-k2.6",
			provider: "opencode-go",
			api: "openai-completions",
			input: ["text"],
			contextWindow: 128000,
			maxTokens: 8192,
		}, {
			scope: "worker",
			metadata: { worker: "w-1" },
		});

		expect(record?.payload.name).toBe(TapeEvent.System.Model);
		expect(record?.payload.data).toEqual({
			scope: "worker",
			model: {
				id: "kimi-k2.6",
				name: "kimi-k2.6",
				provider: "opencode-go",
				api: "openai-completions",
				input: ["text"],
				contextWindow: 128000,
				maxTokens: 8192,
			},
			metadata: { worker: "w-1" },
		});
	});

	test("setTools records tool definitions without execute functions", async () => {
		const { tape } = makeTape();

		const record = await tape.setTools([
			{
				name: "send_to_user",
				label: "Send To User",
				description: "Send progress.",
				parameters: { type: "object", properties: { content: { type: "string" } } },
				execute: async () => ({ content: [] }),
			},
		], {
			scope: "worker",
			metadata: { worker: "w-1" },
		});

		expect(record?.payload.name).toBe(TapeEvent.System.Tools);
		expect(record?.payload.data).toMatchObject({
			scope: "worker",
			metadata: { worker: "w-1" },
		});
		const tool = ((record?.payload.data as any).tools as any[])[0];
		expect(tool).toMatchObject({
			name: "send_to_user",
			label: "Send To User",
			description: "Send progress.",
			parameters: { type: "object", properties: { content: { type: "string" } } },
		});
		expect(tool.execute).toBeUndefined();
	});

	test("setTools omits missing tool parameters instead of recording undefined strings", async () => {
		const { tape } = makeTape();

		const record = await tape.setTools([{ name: "noop" }]);

		expect(((record?.payload.data as any).tools as any[])[0]).toEqual({ name: "noop" });
	});

	test("records generic LLM context, policy, runtime, status, and summary", async () => {
		const { tape, entries } = makeTape();

		await tape.setContext({ conversationId: "c-1", locale: "zh-CN" }, {
			scope: "conversation",
			ifChanged: true,
		});
		await tape.setPolicy({ streaming: true, maxTurns: 8 }, {
			scope: "conversation",
			metadata: { conversationId: "c-1" },
			ifChanged: true,
		});
		await tape.setRuntime({ sandboxed: false, client: "test" }, {
			scope: "conversation",
			metadata: { conversationId: "c-1" },
			ifChanged: true,
		});
		await tape.setStatus("running", {
			scope: "conversation",
			metadata: { conversationId: "c-1" },
			ifChanged: true,
		});
		await tape.recordSummary("user prefers concise Chinese replies", {
			scope: "conversation",
			range: { from: 1, to: 4 },
			metadata: { conversationId: "c-1" },
			ifChanged: true,
		});

		expect(entries.map((entry) => entry.kind === "event" ? entry.payload.name : "")).toEqual([
			TapeEvent.System.Context,
			TapeEvent.System.Policy,
			TapeEvent.System.Runtime,
			TapeEvent.System.Status,
			TapeEvent.System.Summary,
		]);
		expect(entries[0]?.payload.data).toEqual({
			scope: "conversation",
			context: { conversationId: "c-1", locale: "zh-CN" },
			metadata: {},
		});
		expect(await tape.contextStatus({ conversationId: "c-1", locale: "zh-CN" }, {
			scope: "conversation",
		})).toBe("current");
		expect(await tape.summaryStatus("changed", {
			scope: "conversation",
			metadata: { conversationId: "c-1" },
		})).toBe("changed");
	});

	test("loadContext delegates context selection to the configured manager", async () => {
		const { tape, entries } = makeTape({
			contextManager: {
				select({ entries, maxEntries, reason, metadata }) {
					expect(maxEntries).toBe(2);
					expect(reason).toBe("llm-call");
					expect(metadata).toEqual({ turn: 3 });
					return entries.filter((entry) => entry.kind === "message").slice(0, maxEntries);
				},
			},
		});
		entries.push(
			entry(1, { kind: "event", payload: { name: "ignored" }, meta: {}, date: "2026-01-01T00:00:00.000Z" }),
			entry(2, { kind: "message", payload: { role: "user", content: "a" }, meta: {}, date: "2026-01-01T00:00:01.000Z" }),
			entry(3, { kind: "message", payload: { role: "assistant", content: [{ type: "text", text: "b" }] }, meta: {}, date: "2026-01-01T00:00:02.000Z" }),
		);

		const context = await tape.loadContext({ maxEntries: 2, reason: "llm-call", metadata: { turn: 3 } });

		expect(context.map((item) => item.id)).toEqual([2, 3]);
	});

	test("updateSummary delegates summary creation to the configured manager", async () => {
		const { tape, entries } = makeTape({
			summaryManager: {
				summarize({ entries, context, range, scope, metadata, reason }) {
					expect(entries).toHaveLength(2);
					expect(context.map((item) => item.id)).toEqual([2]);
					expect(range).toEqual({ from: 2, to: 2 });
				expect(scope).toBe("conversation");
					expect(metadata).toEqual({ conversationId: "c-1" });
					expect(reason).toBe("checkpoint");
					return {
						summary: { text: "latest assistant says b", throughEntryId: 2 },
						metadata: { conversationId: "c-1", generatedBy: "test" },
					};
				},
			},
		});
		entries.push(
			entry(1, { kind: "message", payload: { role: "user", content: "a" }, meta: {}, date: "2026-01-01T00:00:00.000Z" }),
			entry(2, { kind: "message", payload: { role: "assistant", content: [{ type: "text", text: "b" }] }, meta: {}, date: "2026-01-01T00:00:01.000Z" }),
		);

		const record = await tape.updateSummary({
			scope: "conversation",
			metadata: { conversationId: "c-1" },
			context: [entries[1]!],
			reason: "checkpoint",
			ifChanged: true,
		});

		expect(record?.payload.name).toBe(TapeEvent.System.Summary);
		expect(record?.payload.data).toEqual({
			scope: "conversation",
			range: { from: 2, to: 2 },
			summary: { text: "latest assistant says b", throughEntryId: 2 },
		metadata: { conversationId: "c-1", generatedBy: "test" },
	});
	});

	test("emits lifecycle hooks for append, context, system records, and summaries", async () => {
		const events: string[] = [];
		const { tape } = makeTape({
			hooks: {
				onEvent(event) {
					events.push(`event:${event.type}`);
				},
				onAppend(event) {
					events.push(`append:${event.entries.length}`);
				},
				onContextLoaded(event) {
					events.push(`context:${event.context.length}`);
				},
				onSystemRecorded(event) {
					events.push(`system:${event.name}`);
				},
				onSummaryRecorded(event) {
					events.push(`summary:${event.entry.id}`);
				},
				onSummaryUpdated(event) {
					events.push(`updated:${event.entry?.id ?? "none"}`);
				},
			},
			summaryManager: {
				summarize: () => "summary from hook test",
			},
		});

		await tape.setModel({ id: "m" });
		await tape.loadContext({ maxEntries: 1 });
		await tape.updateSummary();

		expect(events).toEqual([
			"event:append",
			"append:1",
			"event:system:recorded",
			"system:model",
			"event:context:loaded",
			"context:1",
			"event:context:loaded",
			"context:1",
			"event:append",
			"append:1",
			"event:summary:recorded",
			"summary:2",
			"event:summary:updated",
			"updated:2",
		]);
	});

	test("hook errors are isolated from append by default and can be observed", async () => {
		const errors: string[] = [];
		const { tape, entries } = makeTape({
			hooks: {
				onAppend() {
					throw new Error("summary backend down");
				},
				onHookError(error, event) {
					errors.push(`${event.type}:${(error as Error).message}`);
				},
			},
		});

		await tape.recordUserMessage("hello");

		expect(entries).toHaveLength(1);
		expect(errors).toEqual(["append:summary backend down"]);
	});

	test("hook errors can be configured to fail the caller", async () => {
		const { tape, entries } = makeTape({
			hookErrorPolicy: "throw",
			hooks: {
				onAppend() {
					throw new Error("strict hook failure");
				},
			},
		});

		await expect(tape.recordUserMessage("hello")).rejects.toThrow("strict hook failure");
		expect(entries).toHaveLength(1);
	});

	test("built-in context strategies live outside Tape and can be plugged in", async () => {
		const { tape, entries } = makeTape({
			contextManager: createSummaryContextManager({ maxEntries: 2 }),
		});
		entries.push(
			entry(1, { kind: "message", payload: { role: "user", content: "a" }, meta: {}, date: "2026-01-01T00:00:00.000Z" }),
			entry(2, { kind: "event", payload: { name: TapeEvent.System.Summary, data: { summary: "a so far" } }, meta: {}, date: "2026-01-01T00:00:01.000Z" }),
			entry(3, { kind: "message", payload: { role: "assistant", content: [{ type: "text", text: "b" }] }, meta: {}, date: "2026-01-01T00:00:02.000Z" }),
			entry(4, { kind: "message", payload: { role: "user", content: "c" }, meta: {}, date: "2026-01-01T00:00:03.000Z" }),
		);

		expect((await createTailContextManager({ maxEntries: 2 }).select({
			tape,
			entries,
			maxEntries: 10,
		})).map((item) => item.id)).toEqual([3, 4]);
		expect((await tape.loadContext({ maxEntries: 10 })).map((item) => item.id)).toEqual([2, 3, 4]);
	});

	test("auto summary strategy reacts to append events", async () => {
		const { tape, entries } = makeTape({
			summaryManager: createExternalSummaryManager({
				summarize({ entries }) {
					return { summary: `entries:${entries.length}` };
				},
			}),
			hooks: createAutoSummaryHooks({ everyEntries: 2, minEntries: 2, maxEntries: 2, scope: "conversation" }),
		});

		await tape.recordUserMessage("a", { date: "2026-01-01T00:00:00.000Z" });
		expect(entries).toHaveLength(1);
		await tape.recordAssistantMessage([{ type: "text", text: "b" }], { date: "2026-01-01T00:00:01.000Z" });

		expect(entries).toHaveLength(3);
		expect(entries[2]?.kind).toBe("event");
		expect(entries[2]?.payload).toEqual({
			name: TapeEvent.System.Summary,
			data: { scope: "conversation", range: { from: 1, to: 2 }, summary: "entries:2", metadata: {} },
		});
	});

	test("append threshold strategy reacts to accumulated context length", async () => {
		const calls: Array<{ amount: number; appendedIds: number[]; totalIds: number[] }> = [];
		const { tape } = makeTape({
			hooks: createAppendThresholdHooks({
				threshold: 5,
				measure(entry) {
					return String(entry.payload.content ?? "").length;
				},
				onThreshold({ amount, appended, entries, reset }) {
					calls.push({
						amount,
						appendedIds: appended.map((entry) => entry.id),
						totalIds: entries.map((entry) => entry.id),
					});
					reset();
				},
			}),
		});

		await tape.recordUserMessage("ab", { date: "2026-01-01T00:00:00.000Z" });
		await tape.recordAssistantMessage("cde", { date: "2026-01-01T00:00:01.000Z" });
		await tape.recordUserMessage("xy", { date: "2026-01-01T00:00:02.000Z" });

		expect(calls).toEqual([
			{ amount: 5, appendedIds: [2], totalIds: [1, 2] },
		]);
	});
});

function makeTape(options: Partial<ConstructorParameters<typeof Tape>[0]> = {}): { tape: Tape; entries: TapeEntry[] } {
	const ref = { tapeId: "00000000-0000-4000-8000-000000000001" };
	const entries: TapeEntry[] = [];
	function appendTape(_tape: TapeHandle, entry: TapeEntryDraft): Promise<TapeEntry>;
	function appendTape(_tape: TapeHandle, drafts: TapeEntryDraft[]): Promise<TapeEntry[]>;
	async function appendTape(
		_tape: TapeHandle,
		entryOrEntries: TapeEntryDraft | TapeEntryDraft[],
	): Promise<TapeEntry | TapeEntry[]> {
		if (Array.isArray(entryOrEntries)) {
			return entryOrEntries.map((entry) => appendEntry(entries, entry));
		}
		return appendEntry(entries, entryOrEntries);
	}
	const store: TapeStore = {
		create: async () => ref,
		listTapes: async () => [],
		read: async () => entries,
		readPage: async (_tape, { offset, limit }) => entries.slice(offset, offset + limit),
		append: appendTape,
		reset: async () => { entries.length = 0; },
		delete: async () => { entries.length = 0; },
		info: async (): Promise<TapeInfo> => ({
			id: ref.tapeId,
			entries: entries.length,
			anchors: entries.filter((entry) => entry.kind === "anchor").length,
			lastAnchor: null,
			entriesSinceLastAnchor: entries.length,
			lastTokenUsage: null,
		}),
		handoff: async () => {},
		search: async () => [],
	};
	return { tape: new Tape({ store, ref, ...options }), entries };
}

function entry(id: number, draft: TapeEntryDraft): TapeEntry {
	return { id, ...draft };
}

function appendEntry(entries: TapeEntry[], draft: TapeEntryDraft): TapeEntry {
	const entry = { ...draft, id: entries.length + 1 };
	entries.push(entry);
	return entry;
}
