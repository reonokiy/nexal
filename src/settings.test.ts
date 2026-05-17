import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * settings.ts opens a PGlite DB under `os.homedir()/.nexal/data`. On
 * POSIX `os.homedir()` honours $HOME, so we point HOME at a throwaway
 * temp dir BEFORE the first DB call — the suite never touches the real
 * ~/.nexal. getSharedPglite() resolves the dir lazily on first use, so
 * setting HOME in beforeAll (after the hoisted import) is in time.
 */

let realHome: string | undefined;

beforeAll(() => {
	realHome = process.env.HOME;
	process.env.HOME = mkdtempSync(join(tmpdir(), "nexal-settings-test-"));
});

afterAll(async () => {
	const { closeSettings } = await import("./settings.ts");
	await closeSettings();
	if (realHome === undefined) delete process.env.HOME;
	else process.env.HOME = realHome;
});

describe("channel config helpers", () => {
	test("save → loadChannelConfig round-trips the JSON bucket", async () => {
		const s = await import("./settings.ts");
		await s.saveChannelConfig("telegram", { enabled: true, botToken: "A", allowFrom: ["x"] });
		expect(await s.loadChannelConfig("telegram")).toEqual({
			enabled: true,
			botToken: "A",
			allowFrom: ["x"],
		});
		expect(await s.loadChannelConfig("nope")).toBeNull();
	});

	test("loadAllChannelConfigs returns only channel:* keys, prefix stripped", async () => {
		const s = await import("./settings.ts");
		await s.saveChannelConfig("github", { enabled: true, token: "T" });
		await s.setSetting("model:provider", "anthropic"); // non-channel key

		const all = await s.loadAllChannelConfigs();
		expect(all.telegram).toBeDefined();
		expect(all.github).toEqual({ enabled: true, token: "T" });
		expect(Object.keys(all)).not.toContain("model:provider");
		expect(Object.keys(all)).not.toContain("model");
	});

	test("deleteChannelConfig removes the row", async () => {
		const s = await import("./settings.ts");
		await s.saveChannelConfig("cron", { enabled: true });
		expect(await s.loadChannelConfig("cron")).not.toBeNull();
		await s.deleteChannelConfig("cron");
		expect(await s.loadChannelConfig("cron")).toBeNull();
		expect((await s.loadAllChannelConfigs()).cron).toBeUndefined();
	});

	test("onChannelConfigChange fires on save & delete, and unsubscribes", async () => {
		const s = await import("./settings.ts");
		let hits = 0;
		const off = s.onChannelConfigChange(() => hits++);

		await s.saveChannelConfig("heartbeat", { enabled: true });
		expect(hits).toBe(1);
		await s.deleteChannelConfig("heartbeat");
		expect(hits).toBe(2);

		off();
		await s.saveChannelConfig("heartbeat", { enabled: false });
		expect(hits).toBe(2); // no longer notified
	});
});
