import { describe, expect, mock, test } from "bun:test";

import type { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerRunner } from "../workers/runner.ts";
import { createReportToParentTool } from "./report_to_parent.ts";

const RUNNER = { id: "worker-42" } as unknown as WorkerRunner;

function mockRegistry(
	handler: (id: string, msg: string) => void | Promise<void>,
): WorkerRegistry {
	return {
		async reportToParent(id: string, msg: string) {
			await handler(id, msg);
		},
	} as unknown as WorkerRegistry;
}

describe("createReportToParentTool", () => {
	test("shape is right", () => {
		const tool = createReportToParentTool(
			mockRegistry(() => undefined),
			RUNNER,
		);
		expect(tool.name).toBe("report_to_parent");
		expect(tool.label).toBe("Report To Parent");
		expect(tool.description).toMatch(/upward edge/i);
	});

	test("calls registry.reportToParent with caller id and message", async () => {
		const spy = mock(async () => undefined);
		const tool = createReportToParentTool(mockRegistry(spy), RUNNER);
		await tool.execute("c", { text: "done" } as any);
		expect(spy).toHaveBeenCalledTimes(1);
		expect((spy as any).mock.calls[0]).toEqual(["worker-42", "done"]);
	});

	test("response is [reported] with byte count", async () => {
		const tool = createReportToParentTool(
			mockRegistry(() => undefined),
			RUNNER,
		);
		const r = await tool.execute("c", { text: "finished" } as any);
		expect((r.content[0] as { text: string }).text).toBe("[reported]");
		expect(r.details.bytes).toBe(8);
	});

	test("propagates registry errors (parent-not-found / edge violation)", async () => {
		const tool = createReportToParentTool(
			mockRegistry(() => {
				throw new Error("parent not found");
			}),
			RUNNER,
		);
		await expect(
			tool.execute("c", { text: "hi" } as any),
		).rejects.toThrow(/parent not found/);
	});
});
