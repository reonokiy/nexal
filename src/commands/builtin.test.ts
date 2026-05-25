import { describe, expect, test } from "bun:test";

import { parseConfigureArgs, registerBuiltins } from "./builtin.ts";
import { CommandRegistry } from "./registry.ts";
import type { TapeStore } from "../tape/index.ts";

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

	function setup() {
		const registry = new CommandRegistry();
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
					? [
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
						]
					: [],
			append: async (_tape, entryOrEntries: any) =>
				Array.isArray(entryOrEntries)
					? entryOrEntries.map((entry, index) => ({ ...entry, id: index + 1 }))
					: { ...entryOrEntries, id: 1 },
			reset: async () => {},
			info: async (tape) =>
				tape.tapeId === currentTape.id
					? currentTape
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
});
