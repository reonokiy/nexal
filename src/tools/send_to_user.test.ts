import { describe, expect, mock, test } from "bun:test";

import type { WorkerAgent } from "../workers/agent.ts";
import { createSendToUserTool } from "./send_to_user.ts";

function runnerWithSend(
	send: (text: unknown) => void | Promise<void>,
): WorkerAgent {
	return {
		id: "runner-1",
		kind: "executor",
		lifetime: "persistent",
		sandboxKey: "worker:runner-1",
		execClient: {
			readFile: async () => new Uint8Array([255, 216, 255]),
		},
		async sendToChat(text: unknown) {
			await send(text);
		},
	} as unknown as WorkerAgent;
}

describe("createSendToUserTool", () => {
	test("name/label/description are stable", () => {
		const tool = createSendToUserTool(runnerWithSend(() => undefined));
		expect(tool.name).toBe("send_to_user");
		expect(tool.label).toBe("Send To User");
		expect(tool.description.length).toBeGreaterThan(20);
	});

	test("calls runner.sendToChat with the provided text", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendToUserTool(runnerWithSend(spy));
		await tool.execute("call-1", { content: "milestone: done" } as any);
		expect(spy).toHaveBeenCalledTimes(1);
		expect((spy as any).mock.calls[0][0]).toBe("milestone: done");
	});

	test("returns [sent] + byte count details", async () => {
		const tool = createSendToUserTool(runnerWithSend(() => undefined));
		const r = await tool.execute("c", { content: "hello" } as any);
		expect((r.content[0] as { text: string }).text).toBe("[sent]");
		expect(r.details.bytes).toBe(5);
	});

	test("reads sandbox files and sends them as image content", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendToUserTool(runnerWithSend(spy));
		await tool.execute("c", {
			content: "cat",
			files: [{ path: "/workspace/cat.png" }],
		} as any);
		expect((spy as any).mock.calls[0][0]).toEqual([
			{ type: "text", text: "cat" },
			{ type: "image", data: "/9j/", mimeType: "image/png" },
		]);
	});

	test("empty text still calls sendToChat (the runner can guard)", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendToUserTool(runnerWithSend(spy));
		await tool.execute("c", { content: "" } as any);
		expect(spy).toHaveBeenCalled();
	});
});
