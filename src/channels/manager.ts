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
import { HttpChannel } from "./http.ts";
import { WsChannel } from "./ws.ts";
import { TelegramChannel } from "./telegram/index.ts";
import { HeartbeatChannel } from "./heartbeat.ts";
import { CronChannel } from "./cron.ts";
import { GitHubChannel } from "./github.ts";
import { CommandRegistry } from "../commands/registry.ts";
import { registerBuiltins } from "../commands/builtin.ts";
import { loadAllChannelConfigs, onChannelConfigChange } from "../settings.ts";
import type { IncomingMessage } from "./types.ts";
import type { GatewayClient } from "../gateway/index.ts";
import { createLog } from "../log.ts";

const log = createLog("channels");

/** Channels with built-in defaults that run even with no DB row. */
const ALWAYS_ON = new Set(["http", "ws"]);
const KNOWN = ["http", "ws", "telegram", "heartbeat", "cron", "github"] as const;

const OFF = "<off>";

export interface ChannelManagerConfig {
	/** Shared with AgentPool / WorkerRegistry — mutated in place. */
	channels: Map<string, Channel>;
	/** Dispatch callback wired to AgentPool.handle. */
	onMessage: (msg: IncomingMessage) => void;
	/** Gateway client for sandbox monitoring commands. */
	gateway?: GatewayClient;
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
		this.loadConfigs = cfg.loadConfigs ?? loadAllChannelConfigs;
		this.subscribe = cfg.subscribe ?? onChannelConfigChange;
		this.build = cfg.buildChannelFn ?? ((n, c) => this.buildChannel(n, c));
		registerBuiltins(this.commands, cfg.gateway);
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

	/** Returns a constructed channel, or null if disabled / missing creds. */
	private buildChannel(name: string, cfg: Record<string, unknown>): Channel | null {
		switch (name) {
			case "http":
				return new HttpChannel({
					port: Number(cfg.port ?? 3001),
					host: cfg.host as string | undefined,
					commands: this.commands,
					gateway: this.gateway,
				});
			case "ws":
				return new WsChannel({
					port: Number(cfg.port ?? 3000),
					host: (cfg.host as string | undefined) ?? "0.0.0.0",
					unix: cfg.unix as string | undefined,
					commands: this.commands,
				});
			case "telegram": {
				const botToken = cfg.botToken as string | undefined;
				if (!botToken || cfg.enabled !== true) return null;
				return new TelegramChannel({
					botToken,
					allowFrom: cfg.allowFrom as string[] | undefined,
					allowChats: cfg.allowChats as string[] | undefined,
					commands: this.commands,
				});
			}
			case "heartbeat":
				if (cfg.enabled !== true) return null;
				return new HeartbeatChannel({
					intervalMinutes:
						(cfg.intervalMins as number | undefined) ??
						(cfg.intervalMinutes as number | undefined),
				});
			case "cron":
				if (cfg.enabled !== true) return null;
				return new CronChannel({
					tickIntervalSecs: cfg.tickIntervalSecs as number | undefined,
				});
			case "github": {
				const token = cfg.token as string | undefined;
				if (!token || cfg.enabled !== true) return null;
				return new GitHubChannel({
					token,
					pollIntervalSecs: cfg.pollIntervalSecs as number | undefined,
					reasons: cfg.reasons as string[] | undefined,
					subjectTypes: cfg.subjectTypes as string[] | undefined,
				});
			}
			default:
				log.warn(`unknown channel "${name}" in DB config, ignoring`);
				return null;
		}
	}
}
