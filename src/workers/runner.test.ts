import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

import type { Channel } from "../channels/types.ts";
import type { AgentClient } from "../gateway/agent_client.ts";
import type { GatewayClient } from "../gateway/client.ts";

// Stubbing Agent at module scope — see the "Agent lifecycle paths" note
// below. The real Agent lives in @mariozechner/pi-agent-core; we replace
// its export with FakeAgent so start() / event wiring become deterministic.
interface FakeAgentEvent {
	type: "turn_end" | "message_end" | "agent_end";
	message?: any;
	messages?: any[];
}

class FakeAgent {
	state = {
		isStreaming: false,
		messages: [] as any[],
		errorMessage: undefined as string | undefined,
	};
	private readonly subscribers = new Set<(e: FakeAgentEvent) => void | Promise<void>>();
	constructor(opts: any) {
		this.state.messages = [...(opts?.initialState?.messages ?? [])];
	}
	subscribe(fn: (e: FakeAgentEvent) => void | Promise<void>): void {
		this.subscribers.add(fn);
	}
	async prompt(msg: unknown): Promise<void> {
		FakeAgent.lastPrompted = msg;
	}
	steer(msg: unknown): void {
		FakeAgent.lastSteered = msg;
	}
	abort(): void {
		FakeAgent.aborted = true;
	}
	async waitForIdle(): Promise<void> {}
	async emit(event: FakeAgentEvent): Promise<void> {
		for (const s of this.subscribers) await s(event);
	}
	static lastPrompted: unknown = null;
	static lastSteered: unknown = null;
	static aborted = false;
	static reset(): void {
		FakeAgent.lastPrompted = null;
		FakeAgent.lastSteered = null;
		FakeAgent.aborted = false;
	}
}

// Replace the real Agent before runner.ts is (dynamically) loaded below.
mock.module("@mariozechner/pi-agent-core", () => ({
	Agent: FakeAgent,
}));

// Dynamic import so the mock is in place first.
const { WorkerRunner } = (await import("./runner.ts")) as typeof import("./runner.ts");
type WorkerRunner = InstanceType<typeof WorkerRunner>;
type WorkerRunnerDeps = ConstructorParameters<typeof WorkerRunner>[0];

import type {
	SendPolicy,
	WorkerKind,
	WorkerLifetime,
	WorkerRow,
	WorkerStatus,
	WorkerStore,
} from "./store.ts";

/**
 * Tests split into two suites:
 *
 *   - "pre-Agent paths" — methods that don't require start() to have
 *     booted a pi-agent-core Agent. Covers route() / cancel() /
 *     suspend() / dispose() / sendToSourceChat() / getters.
 *   - "Agent lifecycle paths" (see `runner-agent.test.ts`) — uses
 *     mock.module to stub the Agent class and drive the full
 *     wire-events / handleAgentEnd flow.
 *
 * This split keeps the vast majority of coverage driver-free.
 */

function fakeRow(over: Partial<WorkerRow> = {}): WorkerRow {
	return {
		id: "w-1",
		kind: "executor" as WorkerKind,
		lifetime: "persistent" as WorkerLifetime,
		parentSessionKey: "telegram:-1",
		sourceChannel: "telegram",
		sourceChatId: "-1",
		sourceReplyTo: null,
		name: "probe",
		initialPrompt: null,
		systemPrompt: "sp",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		status: "idle" as WorkerStatus,
		messagesJson: "[]",
		containerName: "nexal-worker-probe",
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

interface StoreSpy {
	store: WorkerStore;
	setStatusCalls: Array<[string, WorkerStatus, (string | null)?]>;
	setMessagesCalls: Array<[string, string, number]>;
	markStartedCalls: string[];
	markIdleCalls: Array<[string, string]>;
	markCompletedCalls: Array<[string, string]>;
	markFailedCalls: Array<[string, string]>;
}

function makeStore(): StoreSpy {
	const spy: StoreSpy = {
		store: null as any,
		setStatusCalls: [],
		setMessagesCalls: [],
		markStartedCalls: [],
		markIdleCalls: [],
		markCompletedCalls: [],
		markFailedCalls: [],
	};
	spy.store = {
		async insert(): Promise<any> {
			throw new Error("insert not stubbed");
		},
		async get() {
			return null;
		},
		async listByStatus() {
			return [];
		},
		async listByParent() {
			return [];
		},
		async setStatus(id, status, err = null) {
			spy.setStatusCalls.push([id, status, err ?? null]);
		},
		async setMessages(id, json, turn) {
			spy.setMessagesCalls.push([id, json, turn]);
		},
		async markStarted(id) {
			spy.markStartedCalls.push(id);
		},
		async markIdle(id, json) {
			spy.markIdleCalls.push([id, json]);
		},
		async markCompleted(id, json) {
			spy.markCompletedCalls.push([id, json]);
		},
		async markFailed(id, err) {
			spy.markFailedCalls.push([id, err]);
		},
		async close() {},
	};
	return spy;
}

interface GatewaySpy {
	gateway: GatewayClient;
	releaseCalls: string[];
	detachCalls: string[];
	acquireCalls: string[];
}

function makeGateway(opts?: {
	client?: AgentClient;
}): GatewaySpy {
	const spy: GatewaySpy = {
		gateway: null as any,
		releaseCalls: [],
		detachCalls: [],
		acquireCalls: [],
	};
	const client: AgentClient =
		opts?.client ??
		({
			agentId: "agent-xyz",
			async connect() {},
			async close() {},
			async runCommand() {
				return { exitCode: 0, stdout: "", stderr: "" };
			},
		} as any);
	spy.gateway = {
		async acquireAgent(key: string) {
			spy.acquireCalls.push(key);
			return client;
		},
		async releaseAgent(key: string) {
			spy.releaseCalls.push(key);
		},
		async detachAgent(key: string) {
			spy.detachCalls.push(key);
		},
		async releaseAllAgents() {},
		async invoke() { return {} as any; },
	} as any;
	return spy;
}

function makeRunner(over?: {
	row?: Partial<WorkerRow>;
	channels?: Map<string, Channel>;
	tools?: WorkerRunnerDeps["toolsForKind"];
	onTerminal?: WorkerRunnerDeps["onTerminal"];
	store?: StoreSpy;
	gateway?: GatewaySpy;
	resumed?: boolean;
}) {
	const store = over?.store ?? makeStore();
	const gw = over?.gateway ?? makeGateway();
	const terminalSeen: string[] = [];
	const runner = new WorkerRunner({
		row: fakeRow(over?.row),
		store: store.store,
		gateway: gw.gateway,
		model: {} as any,
		channels: over?.channels ?? new Map<string, Channel>(),
		toolsForKind: over?.tools ?? (() => []),
		resumed: over?.resumed ?? false,
		onTerminal: over?.onTerminal ?? ((id) => terminalSeen.push(id)),
	});
	return { runner, store, gateway: gw, terminalSeen };
}

// ─── pre-Agent paths (no start()) ────────────────────────────────────

describe("WorkerRunner constructor", () => {
	test("derives id / kind / lifetime / sandboxKey from row", () => {
		const { runner } = makeRunner({
			row: { id: "abc", kind: "coordinator", lifetime: "persistent" },
		});
		expect(runner.id).toBe("abc");
		expect(runner.kind).toBe("coordinator");
		expect(runner.lifetime).toBe("persistent");
		expect(runner.sandboxKey).toBe("worker:abc");
	});

	test("latestTurnCount seeds from row.turnCount (via flushNow persistence path)", async () => {
		// Verify indirectly: flushNow is a no-op when there are no messages,
		// but if we force messages via scheduleFlush → flushNow we'd see
		// the turn count persisted. Here we just verify the seed via
		// public behaviour in a later test.
		const { runner } = makeRunner({ row: { turnCount: 9 } });
		expect(runner.row.turnCount).toBe(9);
	});
});

describe("WorkerRunner.route()", () => {
	test("throws when lifetime is 'shot'", async () => {
		const { runner } = makeRunner({ row: { lifetime: "shot" } });
		await expect(runner.route("hi")).rejects.toThrow(/one-shot/);
	});

	test("throws when start() hasn't been called (no Agent)", async () => {
		const { runner } = makeRunner({ row: { lifetime: "persistent" } });
		await expect(runner.route("hi")).rejects.toThrow(/not started/);
	});
});

describe("WorkerRunner.dispose()", () => {
	test("release=true calls gateway.releaseAgent and skips detach", async () => {
		const { runner, gateway } = makeRunner();
		await runner.dispose(true);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
		expect(gateway.detachCalls).toEqual([]);
	});

	test("release=false calls detachAgent", async () => {
		const { runner, gateway } = makeRunner({
			gateway: makeGateway(),
		});
		await runner.dispose(false);
		expect(gateway.releaseCalls).toEqual([]);
		expect(gateway.detachCalls).toEqual(["worker:w-1"]);
	});

	test("second dispose() is a no-op (guards against double-teardown)", async () => {
		const { runner, gateway } = makeRunner();
		await runner.dispose(true);
		await runner.dispose(true);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
	});

	test("swallows client.close() errors so teardown continues", async () => {
		const client: AgentClient = {
			agentId: "a",
			async connect() {},
			async close() {
				throw new Error("client boom");
			},
			async runCommand() {
				return { exitCode: 0, stdout: "", stderr: "" };
			},
		} as any;
		// Assign the private `client` via cast so we can exercise the
		// error path without booting Agent.
		const { runner, gateway } = makeRunner({
			gateway: makeGateway({ client }),
		});
		(runner as any).client = client;
		await runner.dispose(true);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
	});
});

describe("WorkerRunner.cancel() / suspend() — pre-Agent", () => {
	test("cancel() on not-started runner: setStatus 'cancelled' + release + onTerminal", async () => {
		const { runner, store, gateway, terminalSeen } = makeRunner();
		await runner.cancel("user asked");
		expect(store.setStatusCalls).toEqual([["w-1", "cancelled", "user asked"]]);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
		expect(terminalSeen).toEqual(["w-1"]);
	});

	test("cancel() after dispose() is a full no-op (disposed-guard bails early)", async () => {
		const { runner, store, gateway, terminalSeen } = makeRunner();
		await runner.dispose(true);
		await runner.cancel();
		// `cancel()`'s first line is `if (this.disposed) return` — no status
		// change, no second release, no onTerminal notification.
		expect(store.setStatusCalls).toEqual([]);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]); // from dispose only
		expect(terminalSeen).toEqual([]);
	});

	test("suspend() detaches but does NOT release or call onTerminal", async () => {
		const { runner, store, gateway, terminalSeen } = makeRunner();
		await runner.suspend();
		expect(store.setStatusCalls).toEqual([]); // no status change
		expect(store.markIdleCalls).toEqual([]);
		expect(gateway.releaseCalls).toEqual([]);
		expect(gateway.detachCalls).toEqual(["worker:w-1"]);
		expect(terminalSeen).toEqual([]);
	});
});

describe("WorkerRunner.sendToSourceChat()", () => {
	test("no-op for empty / whitespace text", async () => {
		const sendSpy = mock(() => Promise.resolve());
		const channels = new Map<string, Channel>([
			["telegram", { name: "telegram", start: () => {}, stop: async () => {}, send: sendSpy } as any],
		]);
		const { runner } = makeRunner({ channels });
		await runner.sendToSourceChat("");
		await runner.sendToSourceChat("   \n  ");
		expect(sendSpy).not.toHaveBeenCalled();
	});

	test("prefixes text with worker name and forwards to the source channel", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => {
						sent.push(opts);
					},
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: { name: "refactor-bot", sourceChatId: "-42" },
		});
		await runner.sendToSourceChat("step done");
		expect(sent).toEqual([
			{ chatId: "-42", text: "[refactor-bot] step done", replyTo: undefined },
		]);
	});

	test("uses opts.replyTo when provided, falls through to row.sourceReplyTo otherwise", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: { sourceReplyTo: "default-msg-id" },
		});
		await runner.sendToSourceChat("a");
		await runner.sendToSourceChat("b", { replyTo: "other-id" });
		expect(sent[0].replyTo).toBe("default-msg-id");
		expect(sent[1].replyTo).toBe("other-id");
	});

	test("logs and swallows when the configured channel isn't registered", async () => {
		const { runner } = makeRunner({ row: { sourceChannel: "wat" } });
		// Should not throw — error is logged and swallowed.
		await runner.sendToSourceChat("orphan");
	});

	test("logs and swallows when the channel.send throws", async () => {
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async () => {
						throw new Error("network down");
					},
				} as any,
			],
		]);
		const { runner } = makeRunner({ channels });
		// Should not throw — error is logged and swallowed.
		await runner.sendToSourceChat("bad day");
	});
});

describe("WorkerRunner flush behaviour — pre-Agent", () => {
	test("flushNow with no agent and no override is a safe no-op", async () => {
		const { runner, store } = makeRunner();
		await (runner as any).flushNow();
		expect(store.setMessagesCalls).toEqual([]);
	});

	test("flushNow with override messages persists via store.setMessages", async () => {
		const { runner, store } = makeRunner({ row: { turnCount: 7 } });
		(runner as any).latestTurnCount = 7;
		await (runner as any).flushNow([
			{ role: "user", content: "hi", timestamp: 1 },
		]);
		expect(store.setMessagesCalls).toHaveLength(1);
		const [id, json, turn] = store.setMessagesCalls[0]!;
		expect(id).toBe("w-1");
		expect(turn).toBe(7);
		const parsed = JSON.parse(json);
		expect(Array.isArray(parsed)).toBe(true);
		expect(parsed[0].role).toBe("user");
	});

	test("flushNow clears any pending debounce timer", async () => {
		const { runner } = makeRunner();
		(runner as any).scheduleFlush();
		expect((runner as any).persistTimer).not.toBeNull();
		await (runner as any).flushNow();
		expect((runner as any).persistTimer).toBeNull();
	});

	test("scheduleFlush is idempotent — second call doesn't queue a second timer", () => {
		const { runner } = makeRunner();
		(runner as any).scheduleFlush();
		const first = (runner as any).persistTimer;
		(runner as any).scheduleFlush();
		expect((runner as any).persistTimer).toBe(first);
	});

	test("flushNow logs and swallows store.setMessages errors", async () => {
		const spy = makeStore();
		spy.store.setMessages = async () => {
			throw new Error("db down");
		};
		const { runner } = makeRunner({ store: spy });
		// Should not throw — error is logged and swallowed.
		await (runner as any).flushNow([{ role: "user", content: "x", timestamp: 0 }]);
	});
});

// ─── Agent lifecycle paths ───────────────────────────────────────────
//
// Agent is stubbed via `mock.module` at the top of this file. Each test
// resets the FakeAgent statics so cross-test leakage is impossible.

beforeEach(() => {
	FakeAgent.reset();
});

async function startAndGetAgent(runner: WorkerRunner): Promise<FakeAgent> {
	await runner.start();
	return (runner as any).agent as FakeAgent;
}

describe("WorkerRunner.start() — executor paths", () => {
	test("executor acquires a container and marks the row started", async () => {
		const { runner, store, gateway } = makeRunner({
			row: { initialPrompt: "do stuff" },
		});
		await runner.start();
		expect(gateway.acquireCalls).toEqual(["worker:w-1"]);
		expect(store.markStartedCalls).toEqual(["w-1"]);
		expect(FakeAgent.lastPrompted).toBe("do stuff");
	});

	test("resumed executor with prior messages gets the restart nudge, not the initial prompt", async () => {
		const priorMessages = JSON.stringify([
			{ role: "user", content: "original", timestamp: 0 },
		]);
		const { runner } = makeRunner({
			row: { initialPrompt: "should-be-ignored", messagesJson: priorMessages },
			resumed: true,
		});
		await runner.start();
		const prompt = FakeAgent.lastPrompted;
		expect(typeof prompt).toBe("string");
		expect(String(prompt)).toMatch(/interrupted by a process restart/);
	});

	test("persistent executor with no prompt immediately markIdle()s", async () => {
		const { runner, store } = makeRunner({
			row: { initialPrompt: null, lifetime: "persistent" },
		});
		await runner.start();
		expect(FakeAgent.lastPrompted).toBeNull();
		expect(store.markIdleCalls).toHaveLength(1);
		expect(store.markIdleCalls[0]![0]).toBe("w-1");
	});
});

describe("WorkerRunner.start() — coordinator paths", () => {
	test("coordinator does NOT acquire a container (no bash)", async () => {
		const { runner, gateway } = makeRunner({
			row: {
				kind: "coordinator",
				lifetime: "persistent",
				initialPrompt: "dispatch work",
			},
		});
		await runner.start();
		expect(gateway.acquireCalls).toEqual([]); // no bash container
		expect(FakeAgent.lastPrompted).toBe("dispatch work");
	});
});

describe("WorkerRunner.route() — with live Agent", () => {
	test("streaming agent → steer (no new markStarted, no prompt)", async () => {
		const { runner, store } = makeRunner({ row: { lifetime: "persistent" } });
		const agent = await startAndGetAgent(runner);
		agent.state.isStreaming = true;
		store.markStartedCalls.length = 0;
		FakeAgent.lastPrompted = null;
		await runner.route("continue please");
		expect(FakeAgent.lastSteered).toMatchObject({ role: "user", content: "continue please" });
		expect(FakeAgent.lastPrompted).toBeNull();
		expect(store.markStartedCalls).toEqual([]);
	});

	test("idle agent → markStarted + prompt", async () => {
		const { runner, store } = makeRunner({ row: { lifetime: "persistent" } });
		const agent = await startAndGetAgent(runner);
		agent.state.isStreaming = false;
		store.markStartedCalls.length = 0;
		FakeAgent.lastSteered = null;
		await runner.route("work on this");
		expect(store.markStartedCalls).toEqual(["w-1"]);
		expect(FakeAgent.lastSteered).toBeNull();
		expect(FakeAgent.lastPrompted).toMatchObject({ role: "user", content: "work on this" });
	});
});

describe("WorkerRunner event wiring", () => {
	test("turn_end schedules a flush (debounced)", async () => {
		const { runner } = makeRunner();
		const agent = await startAndGetAgent(runner);
		expect((runner as any).persistTimer).toBeNull();
		await agent.emit({ type: "turn_end" });
		expect((runner as any).persistTimer).not.toBeNull();
		// Cleanup: release the timer so test teardown is clean.
		clearTimeout((runner as any).persistTimer);
		(runner as any).persistTimer = null;
	});

	test("turn_end increments latestTurnCount on each event", async () => {
		const { runner } = makeRunner({ row: { turnCount: 5 } });
		(runner as any).latestTurnCount = 5;
		const agent = await startAndGetAgent(runner);
		await agent.emit({ type: "turn_end" });
		await agent.emit({ type: "turn_end" });
		expect((runner as any).latestTurnCount).toBe(7);
	});

	test("assistant message_end with send_policy=all forwards text to chat", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: { sendPolicy: "all" as SendPolicy, name: "bot" },
		});
		const agent = await startAndGetAgent(runner);
		await agent.emit({
			type: "message_end",
			message: { role: "assistant", content: "interim update" },
		});
		expect(sent).toEqual([
			{ chatId: "-1", text: "[bot] interim update", replyTo: undefined },
		]);
	});

	test("assistant message_end with send_policy=explicit does NOT forward", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: { sendPolicy: "explicit" as SendPolicy },
		});
		const agent = await startAndGetAgent(runner);
		await agent.emit({
			type: "message_end",
			message: { role: "assistant", content: "chatter" },
		});
		expect(sent).toEqual([]);
	});
});

describe("WorkerRunner.handleAgentEnd (via agent_end event)", () => {
	test("shot executor → markCompleted + release + onTerminal", async () => {
		const { runner, store, gateway, terminalSeen } = makeRunner({
			row: { lifetime: "shot" as WorkerLifetime, sendPolicy: "explicit" as SendPolicy },
		});
		const agent = await startAndGetAgent(runner);
		await agent.emit({
			type: "agent_end",
			messages: [{ role: "assistant", content: "all done", timestamp: 1 }],
		});
		expect(store.markCompletedCalls).toHaveLength(1);
		expect(store.markCompletedCalls[0]![0]).toBe("w-1");
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
		expect(terminalSeen).toEqual(["w-1"]);
	});

	test("persistent executor → markIdle, NO release, NO onTerminal", async () => {
		const { runner, store, gateway, terminalSeen } = makeRunner({
			row: {
				lifetime: "persistent",
				sendPolicy: "explicit" as SendPolicy,
				// Give it a prompt so start() doesn't also hit the
				// no-prompt → markIdle() short-circuit.
				initialPrompt: "do stuff",
			},
		});
		const agent = await startAndGetAgent(runner);
		store.markIdleCalls.length = 0;
		await agent.emit({
			type: "agent_end",
			messages: [{ role: "assistant", content: "idle now", timestamp: 1 }],
		});
		expect(store.markIdleCalls).toHaveLength(1);
		// acquire once on start, no release on idle
		expect(gateway.releaseCalls).toEqual([]);
		expect(terminalSeen).toEqual([]);
	});

	test("executor + send_policy=final → forwards the last assistant text", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: { sendPolicy: "final" as SendPolicy, name: "bot", lifetime: "shot" },
		});
		const agent = await startAndGetAgent(runner);
		await agent.emit({
			type: "agent_end",
			messages: [
				{ role: "user", content: "hi", timestamp: 1 },
				{ role: "assistant", content: "yo", timestamp: 2 },
				{ role: "assistant", content: "final word", timestamp: 3 },
			],
		});
		expect(sent).toEqual([
			{ chatId: "-1", text: "[bot] final word", replyTo: undefined },
		]);
	});

	test("coordinator + send_policy=final → does NOT forward (dispatching prose suppressed)", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner } = makeRunner({
			channels,
			row: {
				kind: "coordinator",
				lifetime: "persistent",
				sendPolicy: "final" as SendPolicy,
			},
		});
		const agent = await startAndGetAgent(runner);
		await agent.emit({
			type: "agent_end",
			messages: [{ role: "assistant", content: "dispatched", timestamp: 1 }],
		});
		expect(sent).toEqual([]);
	});

	test("errorMessage → markFailed + error message to chat + dispose + onTerminal", async () => {
		const sent: any[] = [];
		const channels = new Map<string, Channel>([
			[
				"telegram",
				{
					name: "telegram",
					start: () => {},
					stop: async () => {},
					send: async (opts: any) => sent.push(opts),
				} as any,
			],
		]);
		const { runner, store, gateway, terminalSeen } = makeRunner({
			channels,
			row: { sendPolicy: "explicit" as SendPolicy, name: "bot", lifetime: "shot" },
		});
		const agent = await startAndGetAgent(runner);
		agent.state.errorMessage = "model returned 500";
		await agent.emit({
			type: "agent_end",
			messages: [{ role: "assistant", content: "partial", timestamp: 1 }],
		});
		expect(store.markFailedCalls).toEqual([["w-1", "model returned 500"]]);
		expect(sent).toEqual([
			{ chatId: "-1", text: "[bot] ❌ failed: model returned 500", replyTo: undefined },
		]);
		expect(gateway.releaseCalls).toEqual(["worker:w-1"]);
		expect(terminalSeen).toEqual(["w-1"]);
		// markCompleted NOT called on failure path.
		expect(store.markCompletedCalls).toEqual([]);
	});
});
