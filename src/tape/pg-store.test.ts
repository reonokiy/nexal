import { describe, expect, test } from "bun:test";

import { normalizeTapeEntryDraft } from "./pg-store.ts";
import type { TapeEntryDraft } from "./index.ts";

describe("normalizeTapeEntryDraft", () => {
	test("converts legacy tool_call entries into assistant toolCall message content", () => {
		const entry: TapeEntryDraft = {
			kind: "tool_call",
			payload: {
				toolCallId: "call-1",
				toolName: "spawn_oneshot",
				args: { name: "cat-pic", prompt: "find a cat" },
			},
			meta: { source: "test" },
			date: "2026-05-28T01:00:00.000Z",
		};

		expect(normalizeTapeEntryDraft(entry)).toEqual({
			kind: "message",
			payload: {
				role: "assistant",
				content: [{
					type: "toolCall",
					id: "call-1",
					name: "spawn_oneshot",
					arguments: { name: "cat-pic", prompt: "find a cat" },
				}],
				timestamp: Date.parse("2026-05-28T01:00:00.000Z"),
			},
			meta: { source: "test" },
			date: "2026-05-28T01:00:00.000Z",
		});
	});

	test("normalizes tool_call blocks inside assistant message content", () => {
		const entry: TapeEntryDraft = {
			kind: "message",
			payload: {
				role: "assistant",
				content: [
					{ type: "text", text: "working" },
					{
						type: "tool_call",
						toolCallId: "call-2",
						toolName: "bash",
						input: { command: "pwd" },
					},
				],
			},
			meta: {},
			date: "2026-05-28T01:00:00.000Z",
		};

		expect(normalizeTapeEntryDraft(entry).payload.content).toEqual([
			{ type: "text", text: "working" },
			{
				type: "toolCall",
				id: "call-2",
				name: "bash",
				arguments: { command: "pwd" },
			},
		]);
	});
});
