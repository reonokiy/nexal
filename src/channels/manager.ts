/**
 * ChannelManager — owns the channel lifecycle, sourcing configuration
 * exclusively from the DB (settings KV, `channel:<name>` blobs).
 *
 * Hot-reload: `reconcile()` diffs the desired set (from
 * `loadAllChannelConfigs`) against what's running and applies the
 * minimal start/stop/restart. It runs on an initial pass, on every
 * `onChannelConfigChange` notification (immediate, from
 * save/deleteChannelConfig), and on a 5s poll as a safety net (which
 * also keeps the process alive).
 *
 * Correctness hinge: the `channels` Map is shared by reference with
 * AgentPool and WorkerRegistry — they route replies via
 * `channels.get(name).send()`. We mutate that *same* Map in place, so a
 * hot-swapped channel instance is picked up automatically.
 *
 * Note: restarting ws/http closes the old Bun.serve, dropping connected
 * WS clients (they must reconnect). telegram/github `send()` is
 * stateless (keyed by chatId) so a mid-session swap is safe.
 */

import type { Channel } from "./types.ts";
import { buildRegisteredChannel, getRegisteredChannels } from "./factory.ts";
import "./http.ts";
import "./ws.ts";
import "./telegram/channel.ts";
import "./heartbeat.ts";
import "./cron.ts";
import "./github.ts";
import { CommandRegistry } from "../commands/registry.ts";
import { registerBuiltins } from "../commands/builtin.ts";
import { loadAllChannelConfigs, onChannelConfigChange } from "../settings.ts";
import type { IncomingMessage } from "./types.ts";
import type { GatewayClient } from "../gateway/index.ts";
import type { TapeHandle, TapeStore } from "../tape/index.ts";
import { createLog } from "../log.ts";

const log = createLog("channels");

/** Channels with built-in defaults that run even with no DB row. */
const ALWAYS_ON = new Set(["http", "ws"]);
const KNOWN = getRegisteredChannels() as readonly string[];

const OFF = "<off>";

export interface ChannelManagerConfig {
	/** Shared with AgentPool / WorkerRegistry — mutated in place. */
	channels: Map<string, Channel>;
	/** Dispatch callback wired to AgentPool.handle. */
	onMessage: (msg: IncomingMessage) => void;
	/** Gateway client for sandbox monitoring commands. */
	gateway?: GatewayClient;
	/** Tape store for tape browsing commands. */
	tapeStore?: TapeStore;
	/** Resolve the persisted tape id for a chat session key. */
	getTapeRef?: (sessionKey: string) => Promise<TapeHandle | null>;
	/** Test seam — defaults to settings.loadAllChannelConfigs. */
	loadConfigs?: () => Promise<Record<string, Record<string, unknown>>>;
	/** Test seam — defaults to settings.onChannelConfigChange. */
	subscribe?: (fn: () => void) => () => void;
	/** Test seam — defaults to the built-in channel factory. */
	buildChannelFn?: (name: string, cfg: Record<string, unknown>) => Channel | null;
}

export class ChannelManager {
	private readonly channels: Map<string, Channel>;
	private readonly onMessage: (msg: IncomingMessage) => void;
	private readonly loadConfigs: () => Promise<Record<string, Record<string, unknown>>>;
	private readonly subscribe: (fn: () => void) => () => void;
	private readonly build: (name: string, cfg: Record<string, unknown>) => Channel | null;
	private readonly commands = new CommandRegistry();
	private readonly gateway: GatewayClient | undefined;
	private readonly tapeStore: TapeStore | undefined;
	private readonly getTapeRef:
		| ((sessionKey: string) => Promise<TapeHandle | null>)
		| undefined;
	/** name → hash of the applied config (or OFF when intentionally not running). */
	private readonly applied = new Map<string, string>();

	private reconciling = false;
	private rerun = false;
	private stopped = false;
	private timer: ReturnType<typeof setInterval> | null = null;
	private unsubscribe: (() => void) | null = null;
	private resolveStopped: (() => void) | null = null;

	constructor(cfg: ChannelManagerConfig) {
		this.channels = cfg.channels;
		this.onMessage = cfg.onMessage;
		this.gateway = cfg.gateway;
		this.tapeStore = cfg.tapeStore;
		this.getTapeRef = cfg.getTapeRef;
		this.loadConfigs = cfg.loadConfigs ?? loadAllChannelConfigs;
		this.subscribe = cfg.subscribe ?? onChannelConfigChange;
		this.build = cfg.buildChannelFn ?? ((n, c) => buildRegisteredChannel(n, c, this.commands, this.gateway));
		registerBuiltins(this.commands, cfg.gateway, this.tapeStore, this.getTapeRef);
	}

	/** Initial reconcile + wire up change notifications & poll timer. */
	async startInitial(): Promise<void> {
		this.unsubscribe = this.subscribe(() => void this.reconcile());
		await this.reconcile();
		this.timer = setInterval(() => {
			if (!this.stopped) void this.reconcile();
		}, 5_000);
	}

	/** Resolves only when `stopAll()` is called — keeps `main` alive. */
	waitUntilStopped(): Promise<void> {
		if (this.stopped) return Promise.resolve();
		return new Promise<void>((resolve) => {
			this.resolveStopped = resolve;
		});
	}

	async stopAll(): Promise<void> {
		this.stopped = true;
		if (this.timer) clearInterval(this.timer);
		this.timer = null;
		this.unsubscribe?.();
		this.unsubscribe = null;
		await Promise.all(
			[...this.channels.values()].map((c) => c.stop().catch(() => undefined)),
		);
		this.channels.clear();
		this.applied.clear();
		this.resolveStopped?.();
		this.resolveStopped = null;
	}

	/** Serialized; a notification during a run schedules exactly one rerun. */
	async reconcile(): Promise<void> {
		if (this.stopped) return;
		if (this.reconciling) {
			this.rerun = true;
			return;
		}
		this.reconciling = true;
		try {
			do {
				this.rerun = false;
				await this.doReconcile();
			} while (this.rerun && !this.stopped);
		} finally {
			this.reconciling = false;
		}
	}

	private async doReconcile(): Promise<void> {
		let all: Record<string, Record<string, unknown>>;
		try {
			all = await this.loadConfigs();
		} catch (err) {
			log.error("failed to load channel configs from DB, starting always-on channels with defaults", err);
			all = {};
		}

		for (const name of KNOWN) {
			if (this.stopped) return;
			const bucket = all[name] ?? (ALWAYS_ON.has(name) ? {} : undefined);
			const channel = bucket !== undefined ? this.build(name, bucket) : null;
			const hash = channel ? JSON.stringify(bucket) : OFF;
			if (this.applied.get(name) === hash) continue;

			// Tear down the running instance (config changed, or now off).
			const running = this.channels.get(name);
			if (running) {
				await running.stop().catch((err) =>
					log.error(`stopping channel "${name}" failed`, err),
				);
				this.channels.delete(name);
			}

			if (channel) {
				this.channels.set(name, channel);
				void channel.start(this.onMessage).catch((err) =>
					log.error(`channel "${name}" start failed`, err),
				);
				log.info(`channel "${name}" ${running ? "restarted" : "started"}`);
			} else if (running) {
				log.info(`channel "${name}" stopped (disabled / config removed)`);
			}
			this.applied.set(name, hash);
		}
	}

}
