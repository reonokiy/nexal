import { describe, expect, mock, test } from "bun:test";

import type { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerRow } from "../workers/store.ts";
import { createDispatcherTools, type DispatcherCtx } from "./worker.ts";

const CTX: DispatcherCtx = {
	parentSessionKey: "telegram:-1001",
	sourceChannel: "telegram",
	sourceChatId: "-1001",
	sourceReplyTo: "msg-42",
};

function mockRegistry(): WorkerRegistry & {
	spawn: ReturnType<typeof mock>;
	routeFromCaller: ReturnType<typeof mock>;
	get: ReturnType<typeof mock>;
	listForParent: ReturnType<typeof mock>;
	cancel: ReturnType<typeof mock>;
} {
	const fakeRow = (over?: Partial<WorkerRow>): WorkerRow => ({
		id: "rowid",
		kind: "executor",
		lifetime: "persistent",
		parentSessionKey: CTX.parentSessionKey,
		sourceChannel: CTX.sourceChannel,
		sourceChatId: CTX.sourceChatId,
		sourceReplyTo: null,
		name: "sample",
		initialPrompt: null,
		systemPrompt: "prompt",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		status: "spawning",
		messagesJson: "[]",
		containerName: "nexal-worker-sample",
		createdAt: 1000,
		startedAt: null,
		updatedAt: 1000,
		completedAt: null,
		error: null,
		turnCount: 0,
		sendPolicy: "explicit",
		...over,
	});
	return {
		spawn: mock(async (req: any) => fakeRow({ id: `spawned-${req.name}`, kind: req.kind, lifetime: req.lifetime })),
		routeFromCaller: mock(async () => undefined),
		get: mock(async (id: string) => (id === "known" ? fakeRow({ id: "known" }) : null)),
		listForParent: mock(async () => [fakeRow({ id: "a" }), fakeRow({ id: "b" })]),
		cancel: mock(async () => undefined),
	} as any;
}

describe("createDispatcherTools", () => {
	test("returns the seven documented tools in stable order", () => {
		const tools = createDispatcherTools(mockRegistry(), CTX);
		expect(tools.map((t) => t.name)).toEqual([
			"spawn_executor",
			"spawn_shot_task",
			"spawn_coordinator",
			"route_to_agent",
			"list_agents",
			"get_agent",
			"cancel_agent",
		]);
	});

	test("each tool has a non-empty description (helps the LLM pick)", () => {
		const tools = createDispatcherTools(mockRegistry(), CTX);
		for (const t of tools) {
			expect(t.description.length).toBeGreaterThan(20);
		}
	});

	test("spawn_executor calls registry.spawn with kind=executor + lifetime=persistent", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		const spawn = tools.find((t) => t.name === "spawn_executor")!;
		await spawn.execute("call-1", {
			name: "refactor",
			system_prompt: "you refactor",
		} as any);
		expect(reg.spawn).toHaveBeenCalledTimes(1);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.kind).toBe("executor");
		expect(req.lifetime).toBe("persistent");
		expect(req.name).toBe("refactor");
		expect(req.systemPrompt).toBe("you refactor");
		expect(req.parentSessionKey).toBe(CTX.parentSessionKey);
		expect(req.sourceChannel).toBe(CTX.sourceChannel);
		expect(req.sourceChatId).toBe(CTX.sourceChatId);
		expect(req.sourceReplyTo).toBe("msg-42");
		expect(req.sendPolicy).toBe("explicit");
	});

	test("spawn_shot_task calls registry.spawn with kind=executor + lifetime=shot", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "spawn_shot_task")!
			.execute("call-2", { name: "build", prompt: "go build" } as any);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.kind).toBe("executor");
		expect(req.lifetime).toBe("shot");
		expect(req.initialPrompt).toBe("go build");
	});

	test("spawn_coordinator calls registry.spawn with kind=coordinator + lifetime=persistent", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "spawn_coordinator")!
			.execute("call-3", { name: "subcoord", system_prompt: "lead" } as any);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.kind).toBe("coordinator");
		expect(req.lifetime).toBe("persistent");
		expect(req.sendPolicy).toBe("explicit");
	});

	test("route_to_agent enforces caller via routeFromCaller", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "route_to_agent")!
			.execute("call-4", { id: "child", message: "do thing" } as any);
		expect(reg.routeFromCaller).toHaveBeenCalledWith(
			CTX.parentSessionKey,
			"child",
			"do thing",
		);
	});

	test("list_agents asks registry scoped to the caller's subtree", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "list_agents")!.execute("call-5", {} as any);
		expect(reg.listForParent).toHaveBeenCalledWith(CTX.parentSessionKey, 20);
		expect(result.details.count).toBe(2);
	});

	test("get_agent returns null row for unknown id (no throw)", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools
			.find((t) => t.name === "get_agent")!
			.execute("call-6", { id: "unknown" } as any);
		expect(result.details.row).toBeNull();
	});

	test("cancel_agent forwards id to registry.cancel", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "cancel_agent")!
			.execute("call-7", { id: "victim" } as any);
		expect(reg.cancel).toHaveBeenCalledWith("victim");
	});
});
