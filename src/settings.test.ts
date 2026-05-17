import { afterAll, beforeAll, describe, expect, test } from "bun:test";

/**
 * PGlite was removed — settings.ts now needs a real Postgres. These
 * tests run only when a disposable test DB is provided (same convention
 * as scripts/smoke-worker-store.ts); otherwise the suite skips so
 * `bun test` stays green with zero infrastructure.
 *
 *   NEXAL_TEST_DB=postgres://… bun test src/settings.test.ts
 */

const TEST_DB = process.env.NEXAL_TEST_DB ?? process.env.DATABASE_URL ?? "";
const suite = TEST_DB ? describe : describe.skip;

const TOUCHED_CHANNELS = ["telegram", "github", "cron", "heartbeat"];

beforeAll(async () => {
	if (!TEST_DB) return;
	const { setDbUrl, runMigrations } = await import("./db.ts");
	setDbUrl(TEST_DB);
	await runMigrations();
});

afterAll(async () => {
	if (!TEST_DB) return;
	const s = await import("./settings.ts");
	for (const n of TOUCHED_CHANNELS) await s.deleteChannelConfig(n).catch(() => {});
	await s.deleteSetting("model:provider").catch(() => {});
	const { closeDb } = await import("./db.ts");
	await closeDb();
});

suite("channel config helpers (Postgres)", () => {
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
