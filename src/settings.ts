/**
 * Settings store — simple KV backed by PGlite (embedded Postgres).
 *
 * Used to persist:
 *   - API keys per provider
 *   - Model provider / model ID preferences
 *   - Any other local config that should survive restarts
 *
 * Data lives in `~/.nexal/data/` alongside the worker store.
 */
import { homedir } from "node:os";
import { join } from "node:path";
import { mkdirSync, readFileSync } from "node:fs";
import { isCompiled, extractPgliteAssets } from "./embedded.ts";

let _db: import("@electric-sql/pglite").PGlite | null = null;
let _dbPromise: Promise<import("@electric-sql/pglite").PGlite> | null = null;

/**
 * Shared PGlite instance for the process. Both settings and worker
 * store use the same `~/.nexal/data/` directory — PGlite only allows
 * one connection per directory, so we share the instance.
 */
export async function getSharedPglite(): Promise<import("@electric-sql/pglite").PGlite> {
	if (_db) return _db;
	if (_dbPromise) return _dbPromise;
	_dbPromise = (async () => {
		const { PGlite } = await import("@electric-sql/pglite");
		const dataDir = join(homedir(), ".nexal", "data");
		mkdirSync(dataDir, { recursive: true });

		// In compiled mode, PGlite can't resolve its WASM/data files from
		// $bunfs. Extract them to disk and pass via constructor options.
		let opts: Record<string, unknown> = {};
		if (isCompiled) {
			const libDir = await extractPgliteAssets();
			if (libDir) {
				const fsBundleBytes = readFileSync(join(libDir, "pglite.data"));
				opts = {
					fsBundle: new Blob([fsBundleBytes]),
					pgliteWasmModule: await WebAssembly.compile(
						readFileSync(join(libDir, "pglite.wasm")),
					),
					initdbWasmModule: await WebAssembly.compile(
						readFileSync(join(libDir, "initdb.wasm")),
					),
				};
			}
		}

		const client = new PGlite(dataDir, opts);
		await client.waitReady;
		_db = client;
		_dbPromise = null;
		return client;
	})();
	return _dbPromise;
}

let _settingsReady = false;

async function db(): Promise<import("@electric-sql/pglite").PGlite> {
	const pg = await getSharedPglite();
	if (!_settingsReady) {
		await pg.exec(`
			CREATE TABLE IF NOT EXISTS settings (
				key TEXT PRIMARY KEY,
				value TEXT NOT NULL
			)
		`);
		_settingsReady = true;
	}
	return pg;
}

export async function getSetting(key: string): Promise<string | null> {
	const pg = await db();
	const res = await pg.query<{ value: string }>(
		"SELECT value FROM settings WHERE key = $1",
		[key],
	);
	return res.rows[0]?.value ?? null;
}

export async function setSetting(key: string, value: string): Promise<void> {
	const pg = await db();
	await pg.exec(
		`INSERT INTO settings (key, value) VALUES ('${escSql(key)}', '${escSql(value)}')
		 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value`,
	);
}

export async function deleteSetting(key: string): Promise<void> {
	const pg = await db();
	await pg.query("DELETE FROM settings WHERE key = $1", [key]);
}

export async function closeSettings(): Promise<void> {
	if (_db) {
		await _db.close();
		_db = null;
	}
}

// ── Auth helpers ────────────────────────────────────────────────────

export interface SavedAuth {
	provider: string;
	apiKey: string;
}

export async function saveAuth(auth: SavedAuth): Promise<void> {
	await setSetting(`auth:${auth.provider}`, JSON.stringify(auth));
}

export async function loadAuth(provider: string): Promise<SavedAuth | null> {
	const raw = await getSetting(`auth:${provider}`);
	if (!raw) return null;
	const parsed = JSON.parse(raw) as Partial<SavedAuth> & { apiKey?: string };
	if (!parsed.apiKey) return null;
	return { provider, apiKey: parsed.apiKey };
}

export async function deleteAuth(provider: string): Promise<void> {
	await deleteSetting(`auth:${provider}`);
}

export async function saveModelConfig(provider: string, modelId: string): Promise<void> {
	await setSetting("model:provider", provider);
	await setSetting("model:id", modelId);
}

export async function loadModelConfig(): Promise<{ provider: string; modelId: string } | null> {
	const provider = await getSetting("model:provider");
	const modelId = await getSetting("model:id");
	if (!provider || !modelId) return null;
	return { provider, modelId };
}

// ── Channel config helpers ──────────────────────────────────────────
//
// Channel configuration lives in the DB only (TOML/env `[channel.*]` is
// deprecated). Same JSON-blob-in-KV pattern as auth: key
// `channel:<name>` → the channel's config bucket. Writers fire
// `notifyChannelConfigChanged` so the ChannelManager can hot-reload
// without a poll round-trip.

type ChannelConfigBucket = Record<string, unknown>;

const channelConfigListeners = new Set<() => void>();

/** Subscribe to channel-config writes. Returns an unsubscribe fn. */
export function onChannelConfigChange(fn: () => void): () => void {
	channelConfigListeners.add(fn);
	return () => channelConfigListeners.delete(fn);
}

function notifyChannelConfigChanged(): void {
	for (const fn of channelConfigListeners) {
		try {
			fn();
		} catch {
			// A misbehaving listener must not break the writer.
		}
	}
}

export async function saveChannelConfig(name: string, config: ChannelConfigBucket): Promise<void> {
	await setSetting(`channel:${name}`, JSON.stringify(config));
	notifyChannelConfigChanged();
}

export async function loadChannelConfig(name: string): Promise<ChannelConfigBucket | null> {
	const raw = await getSetting(`channel:${name}`);
	if (!raw) return null;
	return JSON.parse(raw) as ChannelConfigBucket;
}

export async function deleteChannelConfig(name: string): Promise<void> {
	await deleteSetting(`channel:${name}`);
	notifyChannelConfigChanged();
}

export async function loadAllChannelConfigs(): Promise<Record<string, ChannelConfigBucket>> {
	const pg = await db();
	const res = await pg.query<{ key: string; value: string }>(
		"SELECT key, value FROM settings WHERE key LIKE $1",
		["channel:%"],
	);
	const out: Record<string, ChannelConfigBucket> = {};
	for (const row of res.rows) {
		const name = row.key.slice("channel:".length);
		try {
			out[name] = JSON.parse(row.value) as ChannelConfigBucket;
		} catch {
			// Skip a corrupt row rather than crash the whole reconcile.
		}
	}
	return out;
}

// Simple SQL escape for string literals (PGlite doesn't support $N in exec).
function escSql(s: string): string {
	return s.replace(/'/g, "''");
}
