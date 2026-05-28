import { describe, expect, mock, test } from "bun:test";

import type { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerAgent } from "../workers/agent.ts";
import { createSendToParentTool } from "./send_to_parent.ts";

const RUNNER = {
	id: "worker-42",
	execClient: {
		readFile: async () => new Uint8Array([1, 2, 3]),
	},
} as unknown as WorkerAgent;

function mockRegistry(
	handler: (id: string, msg: unknown) => void | Promise<void>,
): WorkerRegistry {
	return {
		async sendToParent(id: string, msg: unknown) {
			await handler(id, msg);
		},
	} as unknown as WorkerRegistry;
}

describe("createSendToParentTool", () => {
	test("shape is right", () => {
		const tool = createSendToParentTool(
			mockRegistry(() => undefined),
			RUNNER,
		);
		expect(tool.name).toBe("send_to_parent");
		expect(tool.label).toBe("Send To Parent");
		expect(tool.description).toMatch(/upward edge/i);
	});

	test("calls registry.sendToParent with caller id and message", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendToParentTool(mockRegistry(spy), RUNNER);
		await tool.execute("c", { content: "done" } as any);
		expect(spy).toHaveBeenCalledTimes(1);
		expect((spy as any).mock.calls[0]).toEqual(["worker-42", "done"]);
	});

	test("response is [reported] with byte count", async () => {
		const tool = createSendToParentTool(
			mockRegistry(() => undefined),
			RUNNER,
		);
		const r = await tool.execute("c", { content: "finished" } as any);
		expect((r.content[0] as { text: string }).text).toBe("[reported]");
		expect(r.details.bytes).toBe(8);
	});

	test("reads sandbox files and forwards them as image content", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendToParentTool(mockRegistry(spy), RUNNER);
		await tool.execute("c", {
			content: "cat attached",
			files: [{ path: "/workspace/cat.jpg" }],
		} as any);
		expect((spy as any).mock.calls[0][1]).toEqual([
			{ type: "text", text: "cat attached" },
			{ type: "image", data: "AQID", mimeType: "image/jpeg" },
		]);
	});

	test("propagates registry errors (parent-not-found / edge violation)", async () => {
		const tool = createSendToParentTool(
			mockRegistry(() => {
				throw new Error("parent not found");
			}),
			RUNNER,
		);
		await expect(
			tool.execute("c", { content: "hi" } as any),
		).rejects.toThrow(/parent not found/);
	});
});
