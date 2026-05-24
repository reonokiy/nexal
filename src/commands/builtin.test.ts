import { describe, expect, test } from "bun:test";

import { parseConfigureArgs } from "./builtin.ts";

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
