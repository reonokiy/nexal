import { describe, expect, test } from "bun:test";

import {
	RUNTIME_CONTEXT_EVENT,
	hasRuntimeContextEntry,
	runtimeContextRecord,
	runtimeContextStatus,
} from "./runtime-context.ts";

describe("runtime context tape records", () => {
	test("captures system prompt, model identity, and tool definitions without execute functions", () => {
		const record = runtimeContextRecord({
			scope: "worker",
			systemPrompt: "you are a worker",
			model: {
				id: "kimi-k2.6",
				name: "kimi-k2.6",
				provider: "opencode-go",
				api: "openai-completions",
				input: ["text"],
				contextWindow: 128000,
				maxTokens: 8192,
			} as any,
			tools: [{
				name: "send_to_user",
				label: "Send To User",
				description: "Send a progress message.",
				parameters: { type: "object", properties: { content: { type: "string" } } },
				execute: async () => ({ content: [] }),
			} as any],
			metadata: { name: "worker-1", lifetime: "oneshot" },
		});

		expect(record.kind).toBe("event");
		expect(record.payload.name).toBe(RUNTIME_CONTEXT_EVENT);
		expect(record.meta).toEqual({ internal: true, scope: "worker", change: "created" });
		expect(record.payload.data).toMatchObject({
			scope: "worker",
			systemPrompt: "you are a worker",
			model: {
				id: "kimi-k2.6",
				provider: "opencode-go",
				api: "openai-completions",
			},
			metadata: { name: "worker-1", lifetime: "oneshot" },
		});
		const tool = (record.payload.data as any).tools[0];
		expect(tool).toMatchObject({
			name: "send_to_user",
			label: "Send To User",
			description: "Send a progress message.",
			parameters: { type: "object", properties: { content: { type: "string" } } },
		});
		expect(tool.execute).toBeUndefined();
	});

	test("detects existing runtime context entries", () => {
		const entry = {
			id: 1,
			kind: "event",
			payload: { name: RUNTIME_CONTEXT_EVENT },
			meta: {},
			date: new Date().toISOString(),
		} as const;
		expect(hasRuntimeContextEntry([entry])).toBe(true);
		expect(hasRuntimeContextEntry([{ ...entry, payload: { name: "other" } }])).toBe(false);
	});

	test("detects when the latest runtime context system prompt changed", () => {
		const base = {
			scope: "session" as const,
			systemPrompt: "old prompt",
			model: { id: "m", provider: "p" } as any,
			tools: [],
			metadata: { sessionKey: "ws:default" },
		};
		const existing = { ...runtimeContextRecord(base), id: 1 };
		expect(runtimeContextStatus([existing], base)).toBe("current");
		expect(runtimeContextStatus([existing], { ...base, systemPrompt: "new prompt" })).toBe("changed");
	});

	test("marks appended runtime context updates distinctly", () => {
		const record = runtimeContextRecord({
			scope: "session",
			systemPrompt: "new prompt",
			model: {} as any,
			tools: [],
		}, { change: "changed" });

		expect(record.meta).toMatchObject({
			internal: true,
			scope: "session",
			change: "changed",
		});
	});
});
