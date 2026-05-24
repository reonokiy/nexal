/**
 * Shared Postgres connection — the single `Bun.sql` pool + Drizzle
 * instance used by BOTH the settings KV (`settings.ts`) and the worker
 * store (`workers/store.ts`). PGlite was removed; an external Postgres
 * is now mandatory.
 *
 * The connection string comes from `cfg.workers.url` (set via
 * `setDbUrl`) or `DATABASE_URL`. Missing → hard error.
 *
 * Schema is owned by drizzle-kit migrations in `drizzle/`; `runMigrations`
 * applies them once at startup by reading from the on-disk `./drizzle`
 * folder.
 */
import { drizzle } from "drizzle-orm/bun-sql";
import { migrate } from "drizzle-orm/bun-sql/migrator";
import { join } from "node:path";

import * as schema from "./schema.ts";
import { createLog } from "./log.ts";

const log = createLog("db");

let _configuredUrl: string | null = null;

/** Set the Postgres URL from config (takes precedence over env). */
export function setDbUrl(url: string): void {
	if (url) _configuredUrl = url;
}

function resolveUrl(): string {
	const url = _configuredUrl || process.env.DATABASE_URL || "";
	if (!url) {
		throw new Error(
			"DATABASE_URL (Postgres connection string) is required — " +
				"PGlite support was removed. Set it via env or `[workers] url` " +
				"in ~/.nexal/config.toml, e.g. postgres://user:pw@host:5432/db",
		);
	}
	return url;
}

function build(url: string) {
	const sql = new (Bun as any).SQL(url, { max: 5, prepare: false });
	const db = drizzle(sql, { schema });
	return { sql, db };
}

let _handle: ReturnType<typeof build> | null = null;
let _migrated = false;

/** The shared Drizzle instance. Lazily opens the pool on first call. */
export function getDb() {
	if (!_handle) _handle = build(resolveUrl());
	return _handle.db;
}

/** Apply pending migrations once. Safe to call repeatedly. */
export async function runMigrations(): Promise<void> {
	if (_migrated) return;
	const db = getDb();
	const folder = join(import.meta.dir, "..", "drizzle");
	await migrate(db, { migrationsFolder: folder });
	_migrated = true;
	log.success("database migrations applied");
}

/** Close the shared pool (shutdown). */
export async function closeDb(): Promise<void> {
	if (_handle) {
		await _handle.sql.close();
		_handle = null;
		_migrated = false;
	}
}
