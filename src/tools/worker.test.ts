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

	test("spawn_executor forwards initial_prompt and a custom send_policy", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "spawn_executor")!
			.execute("call", {
				name: "refactor",
				system_prompt: "sp",
				initial_prompt: "start work",
				send_policy: "all",
			} as any);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.initialPrompt).toBe("start work");
		expect(req.sendPolicy).toBe("all");
	});

	test("spawn_shot_task forwards custom send_policy override", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "spawn_shot_task")!
			.execute("call", {
				name: "one",
				prompt: "do it",
				send_policy: "final",
			} as any);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.sendPolicy).toBe("final");
	});

	test("spawn_coordinator hard-codes send_policy='explicit' regardless of input", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		await tools
			.find((t) => t.name === "spawn_coordinator")!
			.execute("call", {
				name: "sub",
				system_prompt: "lead",
				// send_policy is NOT a parameter on spawn_coordinator — but
				// verify the spawned request always carries explicit.
			} as any);
		const req = (reg.spawn as any).mock.calls[0][0];
		expect(req.sendPolicy).toBe("explicit");
	});

	test("spawn_executor text summary carries 'spawned executor (persistent)' + status", async () => {
		const reg = mockRegistry();
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools
			.find((t) => t.name === "spawn_executor")!
			.execute("c", { name: "refactor", system_prompt: "sp" } as any);
		expect(result.content[0]).toMatchObject({ type: "text" });
		const text = (result.content[0] as any).text;
		expect(text).toContain("spawned executor (persistent)");
		expect(text).toContain("status=spawning");
		// id comes from the mock's fakeRow (keyed by input name).
		expect(text).toContain("id=spawned-refactor");
	});

	test("list_agents returns '(no agents)' when the registry is empty for this parent", async () => {
		const reg = mockRegistry();
		(reg as any).listForParent = mock(async () => []);
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools
			.find((t) => t.name === "list_agents")!
			.execute("c", {} as any);
		expect((result.content[0] as any).text).toBe("(no agents)");
		expect(result.details.count).toBe(0);
	});

	test("list_agents output line includes id / kind / lifetime / status / name / age / turns", async () => {
		const reg = mockRegistry();
		(reg as any).listForParent = mock(async () => [
			{
				id: "abc123",
				kind: "executor",
				lifetime: "persistent",
				parentSessionKey: CTX.parentSessionKey,
				sourceChannel: CTX.sourceChannel,
				sourceChatId: CTX.sourceChatId,
				sourceReplyTo: null,
				name: "refactor-bot",
				initialPrompt: null,
				systemPrompt: "p",
				modelProvider: "x",
				modelId: "y",
				status: "running",
				messagesJson: "[]",
				containerName: "c",
				createdAt: Date.now() - 5_000,
				startedAt: null,
				updatedAt: Date.now(),
				completedAt: null,
				error: null,
				turnCount: 7,
				sendPolicy: "explicit",
			},
		]);
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools
			.find((t) => t.name === "list_agents")!
			.execute("c", {} as any);
		const text = (result.content[0] as any).text;
		expect(text).toContain("abc123");
		expect(text).toContain("executor");
		expect(text).toContain("persistent");
		expect(text).toContain("running");
		expect(text).toContain("refactor-bot");
		expect(text).toContain("turns=7");
		expect(text).toMatch(/age=\d+s/);
	});

	test("list_agents includes truncated error when a row has one", async () => {
		const longError = "x".repeat(200);
		const reg = mockRegistry();
		(reg as any).listForParent = mock(async () => [
			{
				id: "e",
				kind: "executor",
				lifetime: "shot",
				parentSessionKey: CTX.parentSessionKey,
				sourceChannel: CTX.sourceChannel,
				sourceChatId: CTX.sourceChatId,
				sourceReplyTo: null,
				name: "bad",
				initialPrompt: "x",
				systemPrompt: "p",
				modelProvider: "x",
				modelId: "y",
				status: "failed",
				messagesJson: "[]",
				containerName: "c",
				createdAt: Date.now(),
				startedAt: null,
				updatedAt: Date.now(),
				completedAt: Date.now(),
				error: longError,
				turnCount: 1,
				sendPolicy: "explicit",
			},
		]);
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools
			.find((t) => t.name === "list_agents")!
			.execute("c", {} as any);
		const text = (result.content[0] as any).text;
		expect(text).toContain("err=");
		// formatted truncation adds an ellipsis char
		expect(text).toContain("…");
		// and stops well before the full 200-char error.
		expect(text.length).toBeLessThan(400);
	});

	test("get_agent prints one-line-per-field status summary + empty transcript note", async () => {
		const reg = mockRegistry();
		(reg as any).get = mock(async () => ({
			id: "known",
			kind: "executor",
			lifetime: "shot",
			parentSessionKey: CTX.parentSessionKey,
			sourceChannel: CTX.sourceChannel,
			sourceChatId: CTX.sourceChatId,
			sourceReplyTo: null,
			name: "x",
			initialPrompt: null,
			systemPrompt: "system!",
			modelProvider: "p",
			modelId: "m",
			status: "completed",
			messagesJson: "[]",
			containerName: "c",
			createdAt: 0,
			startedAt: null,
			updatedAt: 1,
			completedAt: 1,
			error: null,
			turnCount: 3,
			sendPolicy: "final",
		}));
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "get_agent")!.execute("c", {
			id: "known",
		} as any);
		const text = (result.content[0] as any).text;
		expect(text).toContain("id=known");
		expect(text).toContain("kind=executor");
		expect(text).toContain("lifetime=shot");
		expect(text).toContain("status=completed");
		expect(text).toContain("turns=3");
		expect(text).toContain("send_policy=final");
		expect(text).toContain("system_prompt=system!");
		expect(text).toContain("transcript tail");
		expect(text).toContain("(empty)");
	});

	test("get_agent renders the last 3 messages in the transcript tail", async () => {
		const reg = mockRegistry();
		const msgs = [
			{ role: "user", content: "one" },
			{ role: "assistant", content: "two" },
			{ role: "user", content: "three" },
			{ role: "assistant", content: "four" },
		];
		(reg as any).get = mock(async () => ({
			id: "k",
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: CTX.parentSessionKey,
			sourceChannel: CTX.sourceChannel,
			sourceChatId: CTX.sourceChatId,
			sourceReplyTo: null,
			name: "n",
			initialPrompt: null,
			systemPrompt: "sp",
			modelProvider: "p",
			modelId: "m",
			status: "idle",
			messagesJson: JSON.stringify(msgs),
			containerName: "c",
			createdAt: 0,
			startedAt: null,
			updatedAt: 1,
			completedAt: null,
			error: null,
			turnCount: 0,
			sendPolicy: "explicit",
		}));
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "get_agent")!.execute("c", {
			id: "k",
		} as any);
		const text = (result.content[0] as any).text;
		expect(text).not.toContain("user: one"); // beyond the 3-tail window
		expect(text).toContain("assistant: two");
		expect(text).toContain("user: three");
		expect(text).toContain("assistant: four");
	});

	test("get_agent renders assistant content-array with text + toolCall blocks", async () => {
		const reg = mockRegistry();
		const msgs = [
			{
				role: "assistant",
				content: [
					{ type: "text", text: "planning…" },
					{ type: "toolCall", name: "bash" },
				],
			},
		];
		(reg as any).get = mock(async () => ({
			id: "k",
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: CTX.parentSessionKey,
			sourceChannel: CTX.sourceChannel,
			sourceChatId: CTX.sourceChatId,
			sourceReplyTo: null,
			name: "n",
			initialPrompt: null,
			systemPrompt: "sp",
			modelProvider: "p",
			modelId: "m",
			status: "idle",
			messagesJson: JSON.stringify(msgs),
			containerName: "c",
			createdAt: 0,
			startedAt: null,
			updatedAt: 1,
			completedAt: null,
			error: null,
			turnCount: 0,
			sendPolicy: "explicit",
		}));
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "get_agent")!.execute("c", {
			id: "k",
		} as any);
		const text = (result.content[0] as any).text;
		expect(text).toContain("assistant: planning…[tool:bash]");
	});

	test("get_agent survives an unparseable messagesJson blob", async () => {
		const reg = mockRegistry();
		(reg as any).get = mock(async () => ({
			id: "k",
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: CTX.parentSessionKey,
			sourceChannel: CTX.sourceChannel,
			sourceChatId: CTX.sourceChatId,
			sourceReplyTo: null,
			name: "n",
			initialPrompt: null,
			systemPrompt: "sp",
			modelProvider: "p",
			modelId: "m",
			status: "idle",
			messagesJson: "{not valid json",
			containerName: "c",
			createdAt: 0,
			startedAt: null,
			updatedAt: 1,
			completedAt: null,
			error: null,
			turnCount: 0,
			sendPolicy: "explicit",
		}));
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "get_agent")!.execute("c", {
			id: "k",
		} as any);
		expect((result.content[0] as any).text).toContain("(unparseable)");
	});

	test("list_agents formatAge falls through m/h/d scales correctly", async () => {
		const reg = mockRegistry();
		const now = Date.now();
		(reg as any).listForParent = mock(async () => [
			{
				id: "m",
				kind: "executor",
				lifetime: "persistent",
				parentSessionKey: CTX.parentSessionKey,
				sourceChannel: CTX.sourceChannel,
				sourceChatId: CTX.sourceChatId,
				sourceReplyTo: null,
				name: "a",
				initialPrompt: null,
				systemPrompt: "p",
				modelProvider: "x",
				modelId: "y",
				status: "idle",
				messagesJson: "[]",
				containerName: "c",
				// 5 minutes ago
				createdAt: now - 5 * 60 * 1000,
				startedAt: null,
				updatedAt: now,
				completedAt: null,
				error: null,
				turnCount: 0,
				sendPolicy: "explicit",
			},
			{
				id: "h",
				kind: "executor",
				lifetime: "persistent",
				parentSessionKey: CTX.parentSessionKey,
				sourceChannel: CTX.sourceChannel,
				sourceChatId: CTX.sourceChatId,
				sourceReplyTo: null,
				name: "b",
				initialPrompt: null,
				systemPrompt: "p",
				modelProvider: "x",
				modelId: "y",
				status: "idle",
				messagesJson: "[]",
				containerName: "c",
				// 3 hours ago
				createdAt: now - 3 * 60 * 60 * 1000,
				startedAt: null,
				updatedAt: now,
				completedAt: null,
				error: null,
				turnCount: 0,
				sendPolicy: "explicit",
			},
			{
				id: "d",
				kind: "executor",
				lifetime: "persistent",
				parentSessionKey: CTX.parentSessionKey,
				sourceChannel: CTX.sourceChannel,
				sourceChatId: CTX.sourceChatId,
				sourceReplyTo: null,
				name: "c",
				initialPrompt: null,
				systemPrompt: "p",
				modelProvider: "x",
				modelId: "y",
				status: "idle",
				messagesJson: "[]",
				containerName: "c",
				// 2 days ago
				createdAt: now - 2 * 24 * 60 * 60 * 1000,
				startedAt: null,
				updatedAt: now,
				completedAt: null,
				error: null,
				turnCount: 0,
				sendPolicy: "explicit",
			},
		]);
		const tools = createDispatcherTools(reg, CTX);
		const result = await tools.find((t) => t.name === "list_agents")!.execute("c", {} as any);
		const text = (result.content[0] as any).text;
		expect(text).toMatch(/m\s+executor.*age=5m/);
		expect(text).toMatch(/h\s+executor.*age=3h/);
		expect(text).toMatch(/d\s+executor.*age=2d/);
	});
});
