import { afterEach, describe, expect, test } from "bun:test";

import { ChannelManager } from "./manager.ts";
import type { Channel, IncomingMessage } from "./types.ts";

/**
 * ChannelManager.reconcile diff logic is tested in isolation: we inject
 * `loadConfigs` (a mutable fake DB) and `buildChannelFn` (returns fake
 * channels instead of binding real ports / hitting the DB), then drive
 * `reconcile()` directly. `start`/`stop` are spies.
 */

const ALWAYS_ON = new Set(["http", "ws"]);

interface FakeChannel extends Channel {
	startCalls: number;
	stopCalls: number;
	id: number;
}

let nextId = 1;
const built: FakeChannel[] = [];

function fakeBuild(name: string, cfg: Record<string, unknown>): Channel | null {
	// Mirror the real gating shape minimally: always-on always builds;
	// others require enabled === true.
	if (!ALWAYS_ON.has(name) && cfg.enabled !== true) return null;
	const ch: FakeChannel = {
		name,
		id: nextId++,
		startCalls: 0,
		stopCalls: 0,
		async start() {
			ch.startCalls++;
		},
		async send() {},
		async stop() {
			ch.stopCalls++;
		},
	};
	built.push(ch);
	return ch;
}

function newManager(db: Record<string, Record<string, unknown>>) {
	const channels = new Map<string, Channel>();
	const received: IncomingMessage[] = [];
	const mgr = new ChannelManager({
		channels,
		onMessage: (m) => received.push(m),
		loadConfigs: async () => structuredClone(db),
		subscribe: () => () => {},
		buildChannelFn: fakeBuild,
	});
	return { mgr, channels };
}

afterEach(() => {
	built.length = 0;
	nextId = 1;
});

describe("ChannelManager.reconcile", () => {
	test("empty DB → only always-on http/ws are running", async () => {
		const { mgr, channels } = newManager({});
		await mgr.reconcile();
		expect([...channels.keys()].sort()).toEqual(["http", "ws"]);
		expect(channels.size).toBe(2);
	});

	test("adding an enabled telegram config starts it", async () => {
		const db: Record<string, Record<string, unknown>> = {};
		const { mgr, channels } = newManager(db);
		await mgr.reconcile();
		expect(channels.has("telegram")).toBe(false);

		db.telegram = { enabled: true, botToken: "A" };
		await mgr.reconcile();
		const tg = channels.get("telegram") as FakeChannel;
		expect(tg).toBeDefined();
		expect(tg.startCalls).toBe(1);
	});

	test("changing config restarts with a fresh instance", async () => {
		const db = { telegram: { enabled: true, botToken: "A" } } as Record<
			string,
			Record<string, unknown>
		>;
		const { mgr, channels } = newManager(db);
		await mgr.reconcile();
		const first = channels.get("telegram") as FakeChannel;

		db.telegram = { enabled: true, botToken: "B" };
		await mgr.reconcile();
		const second = channels.get("telegram") as FakeChannel;

		expect(first.stopCalls).toBe(1);
		expect(second.id).not.toBe(first.id);
		expect(second.startCalls).toBe(1);
	});

	test("disabling removes the channel and stops it", async () => {
		const db = { cron: { enabled: true, tickIntervalSecs: 5 } } as Record<
			string,
			Record<string, unknown>
		>;
		const { mgr, channels } = newManager(db);
		await mgr.reconcile();
		const cron = channels.get("cron") as FakeChannel;
		expect(cron).toBeDefined();

		db.cron = { enabled: false };
		await mgr.reconcile();
		expect(channels.has("cron")).toBe(false);
		expect(cron.stopCalls).toBe(1);
	});

	test("idempotent: unchanged config does not restart", async () => {
		const db = { github: { enabled: true, token: "T" } } as Record<
			string,
			Record<string, unknown>
		>;
		const { mgr, channels } = newManager(db);
		await mgr.reconcile();
		const gh = channels.get("github") as FakeChannel;
		await mgr.reconcile();
		await mgr.reconcile();
		expect(gh.startCalls).toBe(1);
		expect(gh.stopCalls).toBe(0);
		expect((channels.get("github") as FakeChannel).id).toBe(gh.id);
	});

	test("stopAll stops everything and clears the map", async () => {
		const { mgr, channels } = newManager({ telegram: { enabled: true, botToken: "A" } });
		await mgr.reconcile();
		const all = [...channels.values()] as FakeChannel[];
		expect(all.length).toBeGreaterThan(0);
		await mgr.stopAll();
		expect(channels.size).toBe(0);
		expect(all.every((c) => c.stopCalls === 1)).toBe(true);
	});

	test("concurrent reconcile calls are serialized", async () => {
		const db = { telegram: { enabled: true, botToken: "A" } } as Record<
			string,
			Record<string, unknown>
		>;
		const { mgr, channels } = newManager(db);
		await Promise.all([mgr.reconcile(), mgr.reconcile(), mgr.reconcile()]);
		const tg = channels.get("telegram") as FakeChannel;
		expect(tg.startCalls).toBe(1); // not started 3×
	});
});
