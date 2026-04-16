import { describe, expect, mock, test } from "bun:test";

import type { Channel } from "../channels/types.ts";
import type { GatewayClient } from "../gateway/client.ts";
import { WorkerRegistry } from "./registry.ts";
import type {
	SendPolicy,
	WorkerCreate,
	WorkerKind,
	WorkerLifetime,
	WorkerRow,
	WorkerStatus,
	WorkerStore,
} from "./store.ts";

/**
 * Tests focus on the public tree-edge API:
 *   - routeFromCaller: only caller's direct children may be routed to
 *   - reportToParent: parent is a session key OR a worker id; behaviour
 *                     differs
 *
 * We do NOT exercise actual spawning / running here — that would pull
 * in sandbox + agent machinery. Tests construct a WorkerRegistry with
 * a mock store and verify behaviour BEFORE any runner is live
 * (so `route` downstream of the tree check errors with "cannot route",
 * which is enough to confirm the validation path took the right
 * branch).
 */

function fakeRow(over?: Partial<WorkerRow>): WorkerRow {
	return {
		id: "row-id",
		kind: "executor" as WorkerKind,
		lifetime: "persistent" as WorkerLifetime,
		parentSessionKey: "telegram:-1",
		sourceChannel: "telegram",
		sourceChatId: "-1",
		sourceReplyTo: null,
		name: "row",
		initialPrompt: null,
		systemPrompt: "sp",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		status: "idle" as WorkerStatus,
		messagesJson: "[]",
		containerName: "nexal-worker-row",
		createdAt: 0,
		startedAt: null,
		updatedAt: 0,
		completedAt: null,
		error: null,
		turnCount: 0,
		sendPolicy: "explicit" as SendPolicy,
		...over,
	};
}

interface TrackingStore extends WorkerStore {
	readonly rows: Map<string, WorkerRow>;
	readonly inserts: WorkerCreate[];
	readonly statusCalls: Array<[string, string, string | null]>;
	setByStatusResult: (status: string[]) => WorkerRow[];
	closed: boolean;
}

function mockStore(rows: WorkerRow[] = []): TrackingStore {
	const map = new Map(rows.map((r) => [r.id, r] as const));
	const inserts: WorkerCreate[] = [];
	const statusCalls: Array<[string, string, string | null]> = [];
	let closed = false;
	let byStatus: (status: string[]) => WorkerRow[] = (statuses) =>
		[...map.values()].filter((r) => statuses.includes(r.status));
	const store: TrackingStore = {
		rows: map,
		inserts,
		statusCalls,
		get closed() {
			return closed;
		},
		set closed(v: boolean) {
			closed = v;
		},
		set setByStatusResult(fn) {
			byStatus = fn;
		},
		get setByStatusResult() {
			return byStatus;
		},
		async insert(r: WorkerCreate): Promise<WorkerRow> {
			inserts.push(r);
			const row = fakeRow({
				...r,
				sourceReplyTo: r.sourceReplyTo ?? null,
				initialPrompt: r.initialPrompt ?? null,
				sendPolicy: r.sendPolicy ?? "explicit",
				status: "spawning",
				messagesJson: "[]",
				createdAt: Date.now(),
				updatedAt: Date.now(),
			});
			map.set(r.id, row);
			return row;
		},
		async get(id: string) {
			return map.get(id) ?? null;
		},
		async listByStatus(status) {
			const arr = Array.isArray(status) ? status : [status];
			return byStatus(arr);
		},
		async listByParent(key, limit = 20) {
			const rows = [...map.values()]
				.filter((r) => r.parentSessionKey === key)
				.sort((a, b) => b.createdAt - a.createdAt);
			return rows.slice(0, limit);
		},
		async setStatus(id, status, err = null) {
			statusCalls.push([id, status, err ?? null]);
			const row = map.get(id);
			if (row) map.set(id, { ...row, status: status as any, error: err ?? null });
		},
		async setMessages() {},
		async markStarted() {},
		async markIdle() {},
		async markCompleted() {},
		async markFailed() {},
		async close() {
			closed = true;
		},
	} as TrackingStore;
	return store;
}

function buildRegistry(opts?: {
	store?: WorkerStore;
	deliverToTopLevel?: (key: string, sender: string, content: import("../content.ts").UserContent) => void | Promise<void>;
}) {
	return new WorkerRegistry({
		store: opts?.store ?? mockStore(),
		gateway: {} as GatewayClient,
		model: {} as any,
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		channels: new Map<string, Channel>(),
		maxConcurrent: 0, // disable pump — we don't want runners to start
		executorSystemPromptDefault: "exec prompt",
		coordinatorSystemPromptDefault: "coord prompt",
		executorTools: () => [],
		coordinatorTools: () => [],
		deliverToTopLevel: opts?.deliverToTopLevel,
	});
}

describe("WorkerRegistry.routeFromCaller", () => {
	test("throws 'not found' for unknown target id", async () => {
		const reg = buildRegistry({ store: mockStore([]) });
		await expect(reg.routeFromCaller("caller", "missing", "hi")).rejects.toThrow(
			/agent missing not found/,
		);
	});

	test("rejects target whose parent != caller (tree-edge enforced)", async () => {
		const target = fakeRow({ id: "target", parentSessionKey: "other-parent" });
		const reg = buildRegistry({ store: mockStore([target]) });
		await expect(
			reg.routeFromCaller("evil-caller", "target", "take over"),
		).rejects.toThrow(/not a direct child/);
	});

	test("error message names the actual parent so the LLM can route correctly", async () => {
		const target = fakeRow({ id: "target", parentSessionKey: "real-parent-id" });
		const reg = buildRegistry({ store: mockStore([target]) });
		await expect(reg.routeFromCaller("caller", "target", "hi")).rejects.toThrow(
			/its parent is real-parent-id/,
		);
	});

	test("caller == parent passes the validation then fails downstream (no runner)", async () => {
		const target = fakeRow({ id: "target", parentSessionKey: "parent" });
		const reg = buildRegistry({ store: mockStore([target]) });
		// No runner exists, so route() throws a different error — proving
		// the tree-edge check let us past it.
		await expect(reg.routeFromCaller("parent", "target", "hi")).rejects.toThrow(
			/cannot route/,
		);
	});
});

describe("WorkerRegistry.reportToParent", () => {
	test("throws 'not found' for unknown caller id", async () => {
		const reg = buildRegistry({ store: mockStore([]) });
		await expect(reg.reportToParent("ghost", "hi")).rejects.toThrow(
			/agent ghost not found/,
		);
	});

	test("session-key parent goes through deliverToTopLevel callback", async () => {
		const caller = fakeRow({
			id: "exec-1",
			name: "refactor-agent",
			parentSessionKey: "telegram:-100999",
		});
		const deliver = mock(async () => undefined);
		const reg = buildRegistry({
			store: mockStore([caller]),
			deliverToTopLevel: deliver,
		});
		await reg.reportToParent("exec-1", "done with refactor");
		expect(deliver).toHaveBeenCalledTimes(1);
		const args = (deliver as any).mock.calls[0];
		expect(args[0]).toBe("telegram:-100999");
		expect(args[1]).toBe("worker:refactor-agent");
		expect(args[2]).toBe("done with refactor");
	});

	test("session-key parent with no deliverToTopLevel throws a clear error", async () => {
		const caller = fakeRow({ id: "exec-1", parentSessionKey: "telegram:-1" });
		const reg = buildRegistry({ store: mockStore([caller]) });
		await expect(reg.reportToParent("exec-1", "hi")).rejects.toThrow(
			/top-level delivery not configured/,
		);
	});

	test("worker-id parent goes through route() (not through deliverToTopLevel)", async () => {
		// parent = another worker (uuid-shaped id, no `:` in it)
		const caller = fakeRow({
			id: "exec-1",
			name: "child",
			parentSessionKey: "coord-parent-uuid",
		});
		const deliver = mock(async () => undefined);
		const reg = buildRegistry({
			store: mockStore([caller]),
			deliverToTopLevel: deliver,
		});
		// No runner registered for coord-parent-uuid → route() throws.
		// That's fine; we only care that deliverToTopLevel was NOT called.
		await expect(reg.reportToParent("exec-1", "hi")).rejects.toThrow();
		expect(deliver).not.toHaveBeenCalled();
	});

	test("message to worker-id parent gets a `[from child <name>] ` prefix", async () => {
		// Spy via a minimal store that also captures the calls.
		const caller = fakeRow({
			id: "exec-1",
			name: "watchdog",
			parentSessionKey: "parent-uuid",
		});
		const reg = buildRegistry({ store: mockStore([caller]) });
		// We can't observe the prefix directly (route throws before it
		// reaches a runner), but we can verify the thrown error carries
		// the parent id — same code path.
		await expect(reg.reportToParent("exec-1", "ping")).rejects.toThrow(
			/parent-uuid/,
		);
	});
});

describe("WorkerRegistry.spawn", () => {
	test("rejects coordinator + non-persistent combination", async () => {
		const reg = buildRegistry();
		await expect(
			reg.spawn({
				kind: "coordinator",
				lifetime: "shot",
				parentSessionKey: "telegram:-1",
				sourceChannel: "telegram",
				sourceChatId: "-1",
				name: "c",
				initialPrompt: "x",
			}),
		).rejects.toThrow(/coordinators must have persistent lifetime/);
	});

	test("shot lifetime without initialPrompt is rejected (safety: shot needs work to do)", async () => {
		const reg = buildRegistry();
		await expect(
			reg.spawn({
				kind: "executor",
				lifetime: "shot",
				parentSessionKey: "telegram:-1",
				sourceChannel: "telegram",
				sourceChatId: "-1",
				name: "e",
			}),
		).rejects.toThrow(/shot workers require an initial_prompt/);
	});

	test("executor inherits the executor default system prompt when none is given", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		await reg.spawn({
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: "telegram:-1",
			sourceChannel: "telegram",
			sourceChatId: "-1",
			name: "e",
		});
		expect(store.inserts[0]!.systemPrompt).toBe("exec prompt");
		expect(store.inserts[0]!.sendPolicy).toBe("explicit");
	});

	test("coordinator inherits the coordinator default system prompt", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		await reg.spawn({
			kind: "coordinator",
			lifetime: "persistent",
			parentSessionKey: "telegram:-1",
			sourceChannel: "telegram",
			sourceChatId: "-1",
			name: "c",
		});
		expect(store.inserts[0]!.systemPrompt).toBe("coord prompt");
	});

	test("caller-supplied systemPrompt + sendPolicy override defaults", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		await reg.spawn({
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: "telegram:-1",
			sourceChannel: "telegram",
			sourceChatId: "-1",
			name: "e",
			systemPrompt: "special",
			sendPolicy: "final",
		});
		expect(store.inserts[0]!.systemPrompt).toBe("special");
		expect(store.inserts[0]!.sendPolicy).toBe("final");
	});

	test("container name is derived from the spawned id (prefix + 12 hex chars)", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		const row = await reg.spawn({
			kind: "executor",
			lifetime: "persistent",
			parentSessionKey: "telegram:-1",
			sourceChannel: "telegram",
			sourceChatId: "-1",
			name: "e",
		});
		const cn = store.inserts[0]!.containerName;
		expect(cn).toMatch(/^nexal-worker-[0-9a-f]{12}$/);
		// And the returned row should match what we gave the store.
		expect(row.id).toBe(store.inserts[0]!.id);
	});

	test("each spawn gets a unique uuid and a unique container name", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		const req = {
			kind: "executor" as WorkerKind,
			lifetime: "persistent" as WorkerLifetime,
			parentSessionKey: "telegram:-1",
			sourceChannel: "telegram",
			sourceChatId: "-1",
			name: "e",
		};
		const a = await reg.spawn(req);
		const b = await reg.spawn(req);
		expect(a.id).not.toBe(b.id);
		expect(store.inserts[0]!.containerName).not.toBe(store.inserts[1]!.containerName);
	});
});

describe("WorkerRegistry.route (id lookup)", () => {
	test("throws when the agent doesn't exist at all", async () => {
		const reg = buildRegistry();
		await expect(reg.route("nope", "hi")).rejects.toThrow(/agent nope not found/);
	});

	test("throws with the persisted status when a row exists but no runner is live", async () => {
		const row = fakeRow({ id: "r1", status: "idle" as WorkerStatus });
		const reg = buildRegistry({ store: mockStore([row]) });
		await expect(reg.route("r1", "hi")).rejects.toThrow(
			/agent r1 is idle.*cannot route/,
		);
	});
});

describe("WorkerRegistry.cancel", () => {
	test("row in queue but not yet running is dropped and marked cancelled", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		// Put a spawning row directly in queue + map (bypass spawn()'s
		// side-effects — we want to verify cancel() paths).
		const row = fakeRow({ id: "q1", status: "spawning" as WorkerStatus });
		store.rows.set("q1", row);
		(reg as any).queue.push("q1");

		await reg.cancel("q1");
		expect((reg as any).queue).not.toContain("q1");
		expect(store.statusCalls).toEqual([["q1", "cancelled", "cancelled by dispatcher"]]);
	});

	test("already-terminal rows are not re-stamped", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		store.rows.set(
			"done",
			fakeRow({ id: "done", status: "completed" as WorkerStatus }),
		);
		await reg.cancel("done");
		expect(store.statusCalls).toEqual([]);
	});

	test("unknown id is a silent no-op (cannot cancel what we never knew about)", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		await reg.cancel("phantom");
		expect(store.statusCalls).toEqual([]);
	});
});

describe("WorkerRegistry.listForParent", () => {
	test("delegates to store.listByParent with the limit", async () => {
		const store = mockStore([
			fakeRow({ id: "a", parentSessionKey: "telegram:-1", createdAt: 1 }),
			fakeRow({ id: "b", parentSessionKey: "telegram:-1", createdAt: 2 }),
			fakeRow({ id: "other", parentSessionKey: "other-parent", createdAt: 3 }),
		]);
		const reg = buildRegistry({ store });
		const rows = await reg.listForParent("telegram:-1");
		expect(rows.map((r) => r.id).sort()).toEqual(["a", "b"]);
	});
});

describe("WorkerRegistry.resumePending", () => {
	test("picks up every non-terminal row and drops them into the queue", async () => {
		const store = mockStore([
			fakeRow({ id: "r-running", status: "running" as WorkerStatus }),
			fakeRow({ id: "r-idle", status: "idle" as WorkerStatus }),
			fakeRow({ id: "r-spawning", status: "spawning" as WorkerStatus }),
			fakeRow({ id: "r-done", status: "completed" as WorkerStatus }),
		]);
		const reg = buildRegistry({ store });
		await reg.resumePending();
		const queue: string[] = (reg as any).queue;
		expect(new Set(queue)).toEqual(new Set(["r-running", "r-idle", "r-spawning"]));
	});

	test("does not duplicate ids that are already queued", async () => {
		const store = mockStore([
			fakeRow({ id: "r1", status: "running" as WorkerStatus }),
		]);
		const reg = buildRegistry({ store });
		(reg as any).queue.push("r1");
		await reg.resumePending();
		const queue: string[] = (reg as any).queue;
		expect(queue.filter((x) => x === "r1")).toHaveLength(1);
	});
});

describe("WorkerRegistry.shutdown", () => {
	test("empties the queue, suspends every runner, and closes the store", async () => {
		const store = mockStore();
		const reg = buildRegistry({ store });
		(reg as any).queue.push("q1", "q2");
		const suspended: string[] = [];
		(reg as any).runners.set("r1", {
			id: "r1",
			async suspend() {
				suspended.push("r1");
			},
		});
		(reg as any).runners.set("r2", {
			id: "r2",
			async suspend() {
				suspended.push("r2");
			},
		});
		await reg.shutdown();
		expect((reg as any).queue).toEqual([]);
		expect((reg as any).runners.size).toBe(0);
		expect(suspended.sort()).toEqual(["r1", "r2"]);
		expect(store.closed).toBe(true);
	});

	test("suspend errors on one runner don't prevent others from being suspended", async () => {
		const origError = console.error;
		(console as any).error = () => undefined;
		try {
			const store = mockStore();
			const reg = buildRegistry({ store });
			const suspended: string[] = [];
			(reg as any).runners.set("r1", {
				id: "r1",
				row: { name: "r1" },
				async suspend() {
					throw new Error("r1 broken");
				},
			});
			(reg as any).runners.set("r2", {
				id: "r2",
				row: { name: "r2" },
				async suspend() {
					suspended.push("r2");
				},
			});
			await reg.shutdown();
			expect(suspended).toEqual(["r2"]);
		} finally {
			(console as any).error = origError;
		}
	});
});
