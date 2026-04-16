import { describe, expect, mock, test } from "bun:test";

import type { WorkerRunner } from "../workers/runner.ts";
import { createSendUpdateTool } from "./send_update.ts";

function runnerWithSend(
	send: (text: string) => void | Promise<void>,
): WorkerRunner {
	return {
		id: "runner-1",
		kind: "executor",
		lifetime: "persistent",
		sandboxKey: "worker:runner-1",
		async sendToSourceChat(text: string) {
			await send(text);
		},
	} as unknown as WorkerRunner;
}

describe("createSendUpdateTool", () => {
	test("name/label/description are stable", () => {
		const tool = createSendUpdateTool(runnerWithSend(() => undefined));
		expect(tool.name).toBe("send_update");
		expect(tool.label).toBe("Send Update");
		expect(tool.description.length).toBeGreaterThan(20);
	});

	test("calls runner.sendToSourceChat with the provided text", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendUpdateTool(runnerWithSend(spy));
		await tool.execute("call-1", { text: "milestone: done" } as any);
		expect(spy).toHaveBeenCalledTimes(1);
		expect((spy as any).mock.calls[0][0]).toBe("milestone: done");
	});

	test("returns [sent] + byte count details", async () => {
		const tool = createSendUpdateTool(runnerWithSend(() => undefined));
		const r = await tool.execute("c", { text: "hello" } as any);
		expect((r.content[0] as { text: string }).text).toBe("[sent]");
		expect(r.details.bytes).toBe(5);
	});

	test("empty text still calls sendToSourceChat (the runner can guard)", async () => {
		const spy = mock(async () => undefined);
		const tool = createSendUpdateTool(runnerWithSend(spy));
		await tool.execute("c", { text: "" } as any);
		expect(spy).toHaveBeenCalled();
	});
});
