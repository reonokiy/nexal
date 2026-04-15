/**
 * Nexal config loader — slimmed TS port of `crates/nexal-config`.
 *
 * Sources (lowest → highest priority):
 *   1. Built-in defaults
 *   2. `~/.nexal/config.toml` (optional)
 *   3. Env vars prefixed with `NEXAL_`, `__` as nesting separator
 *
 * Env mapping example:
 *   NEXAL_DEBOUNCE_SECS=2          → debounceSecs
 *   NEXAL_CHANNEL__HEARTBEAT__INTERVAL_MINS=15
 *                                   → channel.heartbeat.intervalMins
 *   NEXAL_ADMINS=alice,bob         → admins = ["alice","bob"]
 *
 * This is intentionally smaller than the Rust version — we only need
 * what the Bun runtime actually consumes. Extend as we port more.
 */

import { homedir } from "node:os";
import { join } from "node:path";

export interface NexalConfig {
	/** Workspace root directory (default: cwd). */
	workspace: string;
	/** Path to SOUL.md persona file (default: ~/.nexal/SOUL.md). */
	soulPath: string;
	/** Admin usernames; checked by `isAdmin`. */
	admins: string[];
	/** Debounce delay after @mention (seconds). */
	debounceSecs: number;
	/** Follow-up delay during active window (seconds). */
	messageDelaySecs: number;
	/** Duration of the active conversation window (seconds). */
	activeWindowSecs: number;
	/** Channel-specific raw config buckets. */
	channel: Record<string, Record<string, unknown>>;
	/** Arbitrary provider overrides (mirrors `[providers.NAME]`). */
	providers: Record<string, Record<string, unknown>>;
	/** Long-running sub-agent task subsystem. */
	workers: WorkersConfig;
	/** nexal-gateway connection. */
	gateway: GatewayConfig;
}

export interface GatewayConfig {
	/** WebSocket URL of the gateway, e.g. `"ws://127.0.0.1:5500"`. */
	url: string;
	/** Shared auth token sent in `gateway/hello`. */
	token: string;
	/** Identifier sent in `gateway/hello` (default: `"nexal-bun"`). */
	clientName: string;
}

export interface WorkersConfig {
	/** Persistence backend. */
	backend: "sqlite" | "postgres";
	/** sqlite: filesystem path; postgres: connection string. */
	url: string;
	/** Global cap on concurrent live workers (each holds a Podman container). */
	maxConcurrent: number;
}

const DEFAULTS: NexalConfig = {
	workspace: process.cwd(),
	soulPath: join(homedir(), ".nexal", "SOUL.md"),
	admins: [],
	debounceSecs: 1,
	messageDelaySecs: 10,
	activeWindowSecs: 60,
	channel: {},
	providers: {},
	workers: {
		backend: "sqlite",
		url: join(homedir(), ".nexal", "workers.db"),
		maxConcurrent: 5,
	},
	gateway: {
		url: "ws://127.0.0.1:5500",
		token: "",
		clientName: "nexal-bun",
	},
};

export async function loadConfig(): Promise<NexalConfig> {
	// 1. defaults (copy so we don't mutate)
	const cfg: NexalConfig = structuredClone(DEFAULTS);

	// 2. TOML file
	const tomlPath = process.env.NEXAL_CONFIG_PATH ?? join(homedir(), ".nexal", "config.toml");
	try {
		const text = await Bun.file(tomlPath).text();
		const parsed = Bun.TOML.parse(text) as Record<string, unknown>;
		applyOverlay(cfg, parsed);
	} catch {
		// Missing / unreadable file is fine — all fields have defaults.
	}

	// 3. env
	applyEnv(cfg, process.env as Record<string, string | undefined>);

	return cfg;
}

export function isAdmin(cfg: NexalConfig, username: string): boolean {
	return cfg.admins.includes(username);
}

// ── Overlay helpers ─────────────────────────────────────────────────

function applyOverlay(cfg: NexalConfig, source: Record<string, unknown>): void {
	if (typeof source.workspace === "string") cfg.workspace = source.workspace;
	if (typeof source.soul_path === "string") cfg.soulPath = source.soul_path;
	if (typeof source.soulPath === "string") cfg.soulPath = source.soulPath;
	if (Array.isArray(source.admins)) cfg.admins = source.admins.map(String);
	if (typeof source.debounce_secs === "number") cfg.debounceSecs = source.debounce_secs;
	if (typeof source.debounceSecs === "number") cfg.debounceSecs = source.debounceSecs;
	if (typeof source.message_delay_secs === "number") cfg.messageDelaySecs = source.message_delay_secs;
	if (typeof source.messageDelaySecs === "number") cfg.messageDelaySecs = source.messageDelaySecs;
	if (typeof source.active_window_secs === "number") cfg.activeWindowSecs = source.active_window_secs;
	if (typeof source.activeWindowSecs === "number") cfg.activeWindowSecs = source.activeWindowSecs;
	if (isObject(source.channel)) cfg.channel = mergeMaps(cfg.channel, source.channel);
	if (isObject(source.providers)) cfg.providers = mergeMaps(cfg.providers, source.providers);
	if (isObject(source.workers)) applyWorkersOverlay(cfg.workers, source.workers);
	if (isObject(source.gateway)) applyGatewayOverlay(cfg.gateway, source.gateway);
}

function applyGatewayOverlay(
	gateway: NexalConfig["gateway"],
	source: Record<string, unknown>,
): void {
	const url = source.url;
	if (typeof url === "string") gateway.url = url;
	const token = source.token;
	if (typeof token === "string") gateway.token = token;
	const clientName = source.clientName ?? source.client_name;
	if (typeof clientName === "string") gateway.clientName = clientName;
}

function applyWorkersOverlay(
	workers: NexalConfig["workers"],
	source: Record<string, unknown>,
): void {
	const backend = source.backend;
	if (backend === "sqlite" || backend === "postgres") workers.backend = backend;
	const url = source.url;
	if (typeof url === "string") workers.url = url;
	const dbPath = source.dbPath ?? source.db_path;
	if (typeof dbPath === "string") workers.url = dbPath;
	const maxC = source.maxConcurrent ?? source.max_concurrent;
	if (typeof maxC === "number") workers.maxConcurrent = maxC;
}

function applyEnv(cfg: NexalConfig, env: Record<string, string | undefined>): void {
	for (const [rawKey, val] of Object.entries(env)) {
		if (val === undefined || !rawKey.startsWith("NEXAL_")) continue;
		const path = rawKey.slice("NEXAL_".length).split("__").map((s) => s.toLowerCase());
		setDeep(cfg, path, coerce(val));
	}
}

function setDeep(cfg: NexalConfig, path: string[], value: unknown): void {
	if (path.length === 0) return;

	// Top-level well-known scalar keys.
	if (path.length === 1) {
		const k = snakeToCamel(path[0]!);
		switch (k) {
			case "workspace":
				if (typeof value === "string") cfg.workspace = value;
				return;
			case "soulPath":
				if (typeof value === "string") cfg.soulPath = value;
				return;
			case "admins":
				cfg.admins = csvList(value);
				return;
			case "debounceSecs":
				if (typeof value === "number") cfg.debounceSecs = value;
				return;
			case "messageDelaySecs":
				if (typeof value === "number") cfg.messageDelaySecs = value;
				return;
			case "activeWindowSecs":
				if (typeof value === "number") cfg.activeWindowSecs = value;
				return;
		}
	}

	// Nested: `channel.NAME.KEY` or `providers.NAME.KEY`.
	if (path[0] === "channel") {
		setNested(cfg.channel, path.slice(1), value);
		return;
	}
	if (path[0] === "providers") {
		setNested(cfg.providers, path.slice(1), value);
		return;
	}
	if (path[0] === "workers" && path.length >= 2) {
		const key = snakeToCamel(path.slice(1).join("_"));
		switch (key) {
			case "backend":
				if (value === "sqlite" || value === "postgres") cfg.workers.backend = value;
				return;
			case "url":
			case "dbPath":
				if (typeof value === "string") cfg.workers.url = value;
				return;
			case "maxConcurrent":
				if (typeof value === "number") cfg.workers.maxConcurrent = value;
				return;
		}
		return;
	}
	if (path[0] === "gateway" && path.length >= 2) {
		const key = snakeToCamel(path.slice(1).join("_"));
		switch (key) {
			case "url":
				if (typeof value === "string") cfg.gateway.url = value;
				return;
			case "token":
				if (typeof value === "string") cfg.gateway.token = value;
				return;
			case "clientName":
				if (typeof value === "string") cfg.gateway.clientName = value;
				return;
		}
		return;
	}

	// Unknown top-level group — accept under a synthetic "extra" bucket.
	// Intentionally dropped for now; add explicit handling as we grow.
}

function setNested(target: Record<string, Record<string, unknown>>, path: string[], value: unknown): void {
	if (path.length < 2) return;
	const name = path[0]!.toLowerCase();
	const leafPath = path.slice(1);
	const bucket = target[name] ?? (target[name] = {});
	writeLeaf(bucket, leafPath, value);
}

function writeLeaf(obj: Record<string, unknown>, path: string[], value: unknown): void {
	let cur: Record<string, unknown> = obj;
	for (let i = 0; i < path.length - 1; i++) {
		const k = snakeToCamel(path[i]!);
		const existing = cur[k];
		const next = isObject(existing) ? (existing as Record<string, unknown>) : {};
		cur[k] = next;
		cur = next;
	}
	cur[snakeToCamel(path[path.length - 1]!)] = value;
}

function mergeMaps(
	a: Record<string, Record<string, unknown>>,
	b: Record<string, unknown>,
): Record<string, Record<string, unknown>> {
	const out: Record<string, Record<string, unknown>> = { ...a };
	for (const [k, v] of Object.entries(b)) {
		if (isObject(v)) out[k] = { ...(out[k] ?? {}), ...v };
	}
	return out;
}

function coerce(v: string): unknown {
	if (v === "true") return true;
	if (v === "false") return false;
	if (v === "null" || v === "") return v;
	if (/^-?\d+$/.test(v)) return Number(v);
	if (/^-?\d*\.\d+$/.test(v)) return Number(v);
	return v;
}

function csvList(value: unknown): string[] {
	if (Array.isArray(value)) return value.map(String);
	if (typeof value !== "string") return [];
	return value.split(",").map((s) => s.trim()).filter(Boolean);
}

function snakeToCamel(s: string): string {
	return s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

function isObject(v: unknown): v is Record<string, unknown> {
	return typeof v === "object" && v !== null && !Array.isArray(v);
}
