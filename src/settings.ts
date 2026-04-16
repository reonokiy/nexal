/**
 * Settings store — simple KV backed by PGlite (embedded Postgres).
 *
 * Used to persist:
 *   - OAuth credentials (provider → { refresh, access, expires })
 *   - Model provider / model ID preferences
 *   - Any other local config that should survive restarts
 *
 * Data lives in `~/.nexal/data/` alongside the worker store.
 */
import { homedir } from "node:os";
import { join } from "node:path";
import { mkdirSync } from "node:fs";

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
		const client = new PGlite(dataDir);
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
	type: "oauth" | "apikey";
	access?: string;
	refresh?: string;
	expires?: number;
	apiKey?: string;
}

export async function saveAuth(auth: SavedAuth): Promise<void> {
	await setSetting(`auth:${auth.provider}`, JSON.stringify(auth));
}

export async function loadAuth(provider: string): Promise<SavedAuth | null> {
	const raw = await getSetting(`auth:${provider}`);
	if (!raw) return null;
	return JSON.parse(raw) as SavedAuth;
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

// Simple SQL escape for string literals (PGlite doesn't support $N in exec).
function escSql(s: string): string {
	return s.replace(/'/g, "''");
}
