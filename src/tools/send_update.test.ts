import { describe, expect, mock, test } from "bun:test";

import type { WorkerAgent } from "../workers/agent.ts";
import { createSendUpdateTool } from "./send_update.ts";

function runnerWithSend(
	send: (text: string) => void | Promise<void>,
): WorkerAgent {
	return {
		id: "runner-1",
		kind: "executor",
		lifetime: "persistent",
		sandboxKey: "worker:runner-1",
		async sendToChat(text: string) {
			await send(text);
		},
	} as unknown as WorkerAgent;
}

describe("createSendUpdateTool", () => {
	test("name/label/description are stable", () => {
		const tool = createSendUpdateTool(runnerWithSend(() => undefined));
		expect(tool.name).toBe("send_update");
		expect(tool.label).toBe("Send Update");
		expect(tool.description.length).toBeGreaterThan(20);
	});

	test("calls runner.sendToChat with the provided text", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendUpdateTool(runnerWithSend(spy));
		await tool.execute("call-1", { content: "milestone: done" } as any);
		expect(spy).toHaveBeenCalledTimes(1);
		expect((spy as any).mock.calls[0][0]).toBe("milestone: done");
	});

	test("returns [sent] + byte count details", async () => {
		const tool = createSendUpdateTool(runnerWithSend(() => undefined));
		const r = await tool.execute("c", { content: "hello" } as any);
		expect((r.content[0] as { content: string }).text).toBe("[sent]");
		expect(r.details.bytes).toBe(5);
	});

	test("empty text still calls sendToChat (the runner can guard)", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendUpdateTool(runnerWithSend(spy));
		await tool.execute("c", { content: "" } as any);
		expect(spy).toHaveBeenCalled();
	});
});
