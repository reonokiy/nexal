import { describe, expect, test } from "bun:test";

import { parseConfigureArgs, registerBuiltins } from "./builtin.ts";
import { CommandRegistry } from "./registry.ts";
import type { TapeEntry, TapeStore } from "../tape/index.ts";

describe("parseConfigureArgs", () => {
	test("parses provider, model, and API key", () => {
		expect(parseConfigureArgs(["google", "gemini-2.5-flash", "AIza-test"])).toEqual({
			ok: true,
			provider: "google",
			modelId: "gemini-2.5-flash",
			apiKey: "AIza-test",
		});
	});

	test("parses optional base URL", () => {
		expect(
			parseConfigureArgs([
				"opencode-go",
				"kimi-k2.6",
				"secret",
				"--base-url",
				"https://example.test/v1",
			]),
		).toEqual({
			ok: true,
			provider: "opencode-go",
			modelId: "kimi-k2.6",
			apiKey: "secret",
			baseUrl: "https://example.test/v1",
		});
	});

	test("allows base URL without changing the API key", () => {
		expect(
			parseConfigureArgs([
				"opencode-go",
				"kimi-k2.6",
				"--url",
				"https://example.test/v1",
			]),
		).toEqual({
			ok: true,
			provider: "opencode-go",
			modelId: "kimi-k2.6",
			apiKey: "",
			baseUrl: "https://example.test/v1",
		});
	});

	test("rejects incomplete input", () => {
		const result = parseConfigureArgs(["google"]);
		expect(result.ok).toBe(false);
		if (!result.ok) expect(result.result.error).toBe("missing provider or model_id");
	});
});

describe("tape commands", () => {
	const currentTape = {
		id: "00000000-0000-4000-8000-000000000001",
		entries: 2,
		anchors: 1,
		lastAnchor: "session/start",
		entriesSinceLastAnchor: 1,
		lastTokenUsage: null,
	};
	const childTape = {
		...currentTape,
		id: "00000000-0000-4000-8000-000000000002",
		entries: 1,
		anchors: 0,
		lastAnchor: null,
		entriesSinceLastAnchor: 1,
	};

	function setup(opts: { includeChildRef?: boolean } = {}) {
		const registry = new CommandRegistry();
		const currentEntries: TapeEntry[] = [
			{
				id: 1,
				kind: "anchor",
				payload: { name: "session/start" },
				meta: {},
				date: "2026-01-01T00:00:00.000Z",
			},
			{
				id: 2,
				kind: "message",
				payload: { role: "user", content: "hello" },
				meta: {},
				date: "2026-01-01T00:00:01.000Z",
			},
		];
		if (opts.includeChildRef) {
			currentEntries.push({
				id: 3,
				kind: "ref",
				payload: {
					ref: {
						type: "tape",
						tapeId: childTape.id,
						relation: "link",
						meta: { name: "worker-1", kind: "executor" },
					},
				},
				meta: { event: "agent_spawn" },
				date: "2026-01-01T00:00:02.000Z",
			});
		}
		const store: TapeStore = {
			create: async () => ({ tapeId: currentTape.id }),
			listTapes: async () => [
				currentTape,
				{
					...currentTape,
					id: "00000000-0000-4000-8000-000000000999",
				},
			],
			read: async (tape) =>
				tape.tapeId === currentTape.id
					? currentEntries
					: tape.tapeId === childTape.id
						? [{
								id: 1,
								kind: "message",
								payload: { role: "assistant", content: "child" },
								meta: {},
								date: "2026-01-01T00:00:03.000Z",
							}]
					: [],
			readPage: async (tape, { offset, limit }) =>
				tape.tapeId === currentTape.id
					? currentEntries.slice(offset, offset + limit)
					: tape.tapeId === childTape.id
						? ([{
								id: 1,
								kind: "message",
								payload: { role: "assistant", content: "child" },
								meta: {},
								date: "2026-01-01T00:00:03.000Z",
							}] satisfies TapeEntry[]).slice(offset, offset + limit)
					: [],
			append: async (_tape, entryOrEntries: any) =>
				Array.isArray(entryOrEntries)
					? entryOrEntries.map((entry, index) => ({ ...entry, id: index + 1 }))
					: { ...entryOrEntries, id: 1 },
			reset: async () => {},
			delete: async () => {},
			info: async (tape) =>
				tape.tapeId === currentTape.id
					? currentTape
					: tape.tapeId === childTape.id
						? childTape
					: { ...currentTape, id: tape.tapeId, entries: 0, anchors: 0, lastAnchor: null },
			handoff: async () => {},
			search: async () => [],
		};
		registerBuiltins(registry, undefined, store, async (key) =>
			key === "ws:default" ? { tapeId: currentTape.id } : null,
		);
		return registry;
	}

	test("lists only the current session tape", async () => {
		const registry = setup();
		const result = await registry.execute(
			"tapes",
			{ channel: "ws", chatId: "default", sender: "test" },
			[],
		);
		expect(result?.error).toBeUndefined();
		expect(result?.data).toEqual({ tapes: [currentTape] });
	});

	test("rejects reading another tape id", async () => {
		const registry = setup();
		const result = await registry.execute(
			"tape",
			{ channel: "ws", chatId: "default", sender: "test" },
			["00000000-0000-4000-8000-000000000999"],
		);
		expect(result?.error).toBe("tape not found");
	});

	test("allows reading tapes referenced from the current tape", async () => {
		const registry = setup({ includeChildRef: true });
		const result = await registry.execute(
			"tape",
			{ channel: "ws", chatId: "default", sender: "test" },
			[childTape.id],
		);
		expect(result?.error).toBeUndefined();
		expect(result?.data).toEqual({ tape: childTape });
	});

	test("deletes referenced child tapes", async () => {
		const registry = setup({ includeChildRef: true });
		const result = await registry.execute(
			"tape_delete",
			{ channel: "ws", chatId: "default", sender: "test" },
			[childTape.id],
		);
		expect(result?.error).toBeUndefined();
		expect(result?.data).toEqual({ tapeId: childTape.id });
	});

	test("refuses deleting the current session tape", async () => {
		const registry = setup();
		const result = await registry.execute(
			"tape_delete",
			{ channel: "ws", chatId: "default", sender: "test" },
			[currentTape.id],
		);
		expect(result?.error).toBe("cannot delete current session tape");
	});
});

describe("sandbox commands", () => {
	test("starts a manual sandbox through the gateway", async () => {
		const registry = new CommandRegistry();
		const gateway = {
			spawnAgent: async (params: any) => ({
				agent_id: "agent-1",
				container_name: `container-${params.name}`,
			}),
			listAgents: async () => ({ agents: [] }),
		};
		registerBuiltins(registry, gateway as any);
		const result = await registry.execute(
			"sandbox_start",
			{ channel: "ws", chatId: "default", sender: "test" },
			["manual-test"],
		);
		expect(result?.error).toBeUndefined();
		expect(result?.data).toEqual({
			agent: { agent_id: "agent-1", container_name: "container-manual-test" },
		});
	});

	test("runs a shell command in a sandbox", async () => {
		const registry = new CommandRegistry();
		const calls: any[] = [];
		const gateway = {
			listAgents: async () => ({ agents: [] }),
			spawnAgent: async () => ({ agent_id: "agent-1", container_name: "container" }),
			request: async (requestMethod: string, requestParams: any) => {
				if (requestMethod !== "agent/invoke") throw new Error(`unexpected ${requestMethod}`);
				const method = requestParams.method as string;
				const params = requestParams.params;
				calls.push({ method, params });
				if (method === "process/start") return { process_id: params.process_id };
				if (method === "process/read") {
					return {
						chunks: [{ seq: 1, stream: "stdout", chunk: new TextEncoder().encode("hello\n") }],
						next_seq: 2,
						exited: true,
						exit_code: 0,
						closed: true,
						failure: null,
					};
				}
				throw new Error(`unexpected ${method}`);
			},
		};
		registerBuiltins(registry, gateway as any);
		const result = await registry.execute(
			"sandbox_exec",
			{ channel: "ws", chatId: "default", sender: "test" },
			["agent-1", "echo hello"],
		);
		expect(result?.error).toBeUndefined();
		expect(result?.data).toMatchObject({
			agentId: "agent-1",
			command: "echo hello",
			stdout: "hello\n",
			exitCode: 0,
			timedOut: false,
		});
		expect(calls[0]).toMatchObject({
			method: "process/start",
			params: {
				argv: ["/bin/bash", "-lc", "echo hello"],
				cwd: "/workspace",
				tty: false,
			},
		});
	});
});
