/**
 * WorkerStore — Drizzle-backed persistence for sub-agent workers.
 *
 * Two drivers are supported:
 *   - `"sqlite"`   → `drizzle-orm/bun-sqlite` on top of `bun:sqlite`
 *   - `"postgres"` → `drizzle-orm/postgres-js` on top of `postgres`
 *
 * `createWorkerStore(cfg)` is the one entry point — `WorkerRegistry` /
 * `WorkerRunner` consume only the `WorkerStore` interface and don't
 * know which driver is wired.
 */
import { Database } from "bun:sqlite";
import { and, desc, eq, inArray } from "drizzle-orm";
import { drizzle as drizzleBun } from "drizzle-orm/bun-sqlite";
import { drizzle as drizzlePg } from "drizzle-orm/postgres-js";
import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import postgres from "postgres";

import * as pgSchema from "./schema-pg.ts";
import * as sqliteSchema from "./schema-sqlite.ts";

export type WorkerKind = "coordinator" | "executor";
export type WorkerLifetime = "persistent" | "shot";
export type WorkerStatus =
	| "spawning"
	| "idle"
	| "running"
	| "completed"
	| "cancelled"
	| "failed";
export type SendPolicy = "explicit" | "final" | "all";

/** Plain row shape returned by the store. Identical across drivers. */
export interface WorkerRow {
	id: string;
	kind: WorkerKind;
	lifetime: WorkerLifetime;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo: string | null;
	name: string;
	initialPrompt: string | null;
	systemPrompt: string;
	modelProvider: string;
	modelId: string;
	status: WorkerStatus;
	messagesJson: string;
	containerName: string;
	createdAt: number;
	startedAt: number | null;
	updatedAt: number;
	completedAt: number | null;
	error: string | null;
	turnCount: number;
	sendPolicy: SendPolicy;
}

export interface WorkerCreate {
	id: string;
	kind: WorkerKind;
	lifetime: WorkerLifetime;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
	name: string;
	initialPrompt?: string | null;
	systemPrompt: string;
	modelProvider: string;
	modelId: string;
	containerName: string;
	sendPolicy?: SendPolicy;
}

export interface WorkerStore {
	readonly backend: "sqlite" | "postgres";
	insert(row: WorkerCreate): Promise<WorkerRow>;
	get(id: string): Promise<WorkerRow | null>;
	listByStatus(status: WorkerStatus | WorkerStatus[]): Promise<WorkerRow[]>;
	listByParent(parentSessionKey: string, limit?: number): Promise<WorkerRow[]>;
	setStatus(id: string, status: WorkerStatus, error?: string | null): Promise<void>;
	setMessages(id: string, messagesJson: string, turnCount: number): Promise<void>;
	markStarted(id: string): Promise<void>;
	markIdle(id: string, messagesJson: string): Promise<void>;
	markCompleted(id: string, messagesJson: string): Promise<void>;
	markFailed(id: string, error: string): Promise<void>;
	close(): Promise<void>;
}

export interface WorkerStoreConfig {
	backend: "sqlite" | "postgres";
	/** sqlite: filesystem path; postgres: connection string. */
	url: string;
}

export async function createWorkerStore(
	cfg: WorkerStoreConfig,
): Promise<WorkerStore> {
	if (cfg.backend === "postgres") return createPgStore(cfg.url);
	return createSqliteStore(cfg.url);
}

// ── SQLite ──────────────────────────────────────────────────────────────

async function createSqliteStore(path: string): Promise<WorkerStore> {
	await mkdir(dirname(path), { recursive: true }).catch(() => undefined);
	const raw = new Database(path, { create: true });
	raw.exec("PRAGMA journal_mode = WAL;");
	raw.exec(sqliteSchema.CREATE_SQL);
	const db = drizzleBun(raw, { schema: sqliteSchema });
	const { workers } = sqliteSchema;

	async function getById(id: string): Promise<WorkerRow | null> {
		const rows = await db.select().from(workers).where(eq(workers.id, id)).all();
		return rows[0] ? castRow(rows[0]) : null;
	}

	return {
		backend: "sqlite",
		async insert(row: WorkerCreate): Promise<WorkerRow> {
			const now = Date.now();
			await db
				.insert(workers)
				.values({
					id: row.id,
					kind: row.kind,
					lifetime: row.lifetime,
					parentSessionKey: row.parentSessionKey,
					sourceChannel: row.sourceChannel,
					sourceChatId: row.sourceChatId,
					sourceReplyTo: row.sourceReplyTo ?? null,
					name: row.name,
					initialPrompt: row.initialPrompt ?? null,
					systemPrompt: row.systemPrompt,
					modelProvider: row.modelProvider,
					modelId: row.modelId,
					status: "spawning",
					messagesJson: "[]",
					containerName: row.containerName,
					createdAt: now,
					startedAt: null,
					updatedAt: now,
					completedAt: null,
					error: null,
					turnCount: 0,
					sendPolicy: row.sendPolicy ?? "explicit",
				})
				.run();
			const out = await getById(row.id);
			if (!out) throw new Error(`insert returned no row for ${row.id}`);
			return out;
		},
		get: getById,
		async listByStatus(status): Promise<WorkerRow[]> {
			const arr = Array.isArray(status) ? status : [status];
			const rows = await db
				.select()
				.from(workers)
				.where(inArray(workers.status, arr))
				.orderBy(workers.createdAt)
				.all();
			return rows.map(castRow);
		},
		async listByParent(parentSessionKey: string, limit = 50): Promise<WorkerRow[]> {
			const rows = await db
				.select()
				.from(workers)
				.where(eq(workers.parentSessionKey, parentSessionKey))
				.orderBy(desc(workers.createdAt))
				.limit(limit)
				.all();
			return rows.map(castRow);
		},
		async setStatus(id, status, error = null): Promise<void> {
			await db
				.update(workers)
				.set({ status, error, updatedAt: Date.now() })
				.where(eq(workers.id, id))
				.run();
		},
		async setMessages(id, messagesJson, turnCount): Promise<void> {
			await db
				.update(workers)
				.set({ messagesJson, turnCount, updatedAt: Date.now() })
				.where(eq(workers.id, id))
				.run();
		},
		async markStarted(id): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "running", startedAt: now, updatedAt: now })
				.where(and(eq(workers.id, id)))
				.run();
		},
		async markIdle(id, messagesJson): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "idle", messagesJson, updatedAt: now })
				.where(eq(workers.id, id))
				.run();
		},
		async markCompleted(id, messagesJson): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({
					status: "completed",
					messagesJson,
					completedAt: now,
					updatedAt: now,
					error: null,
				})
				.where(eq(workers.id, id))
				.run();
		},
		async markFailed(id, error): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "failed", error, completedAt: now, updatedAt: now })
				.where(eq(workers.id, id))
				.run();
		},
		async close(): Promise<void> {
			raw.close();
		},
	};
}

// ── Postgres ────────────────────────────────────────────────────────────

async function createPgStore(url: string): Promise<WorkerStore> {
	const sql = postgres(url, { onnotice: () => undefined });
	await sql.unsafe(pgSchema.CREATE_SQL);
	const db = drizzlePg(sql, { schema: pgSchema });
	const { workers } = pgSchema;

	return {
		backend: "postgres",
		async insert(row: WorkerCreate): Promise<WorkerRow> {
			const now = Date.now();
			const [inserted] = await db
				.insert(workers)
				.values({
					id: row.id,
					kind: row.kind,
					lifetime: row.lifetime,
					parentSessionKey: row.parentSessionKey,
					sourceChannel: row.sourceChannel,
					sourceChatId: row.sourceChatId,
					sourceReplyTo: row.sourceReplyTo ?? null,
					name: row.name,
					initialPrompt: row.initialPrompt ?? null,
					systemPrompt: row.systemPrompt,
					modelProvider: row.modelProvider,
					modelId: row.modelId,
					status: "spawning",
					messagesJson: "[]",
					containerName: row.containerName,
					createdAt: now,
					startedAt: null,
					updatedAt: now,
					completedAt: null,
					error: null,
					turnCount: 0,
					sendPolicy: row.sendPolicy ?? "explicit",
				})
				.returning();
			if (!inserted) throw new Error(`insert returned no row for ${row.id}`);
			return castRow(inserted);
		},
		async get(id): Promise<WorkerRow | null> {
			const rows = await db.select().from(workers).where(eq(workers.id, id));
			return rows[0] ? castRow(rows[0]) : null;
		},
		async listByStatus(status): Promise<WorkerRow[]> {
			const arr = Array.isArray(status) ? status : [status];
			const rows = await db
				.select()
				.from(workers)
				.where(inArray(workers.status, arr))
				.orderBy(workers.createdAt);
			return rows.map(castRow);
		},
		async listByParent(parentSessionKey, limit = 50): Promise<WorkerRow[]> {
			const rows = await db
				.select()
				.from(workers)
				.where(eq(workers.parentSessionKey, parentSessionKey))
				.orderBy(desc(workers.createdAt))
				.limit(limit);
			return rows.map(castRow);
		},
		async setStatus(id, status, error = null): Promise<void> {
			await db
				.update(workers)
				.set({ status, error, updatedAt: Date.now() })
				.where(eq(workers.id, id));
		},
		async setMessages(id, messagesJson, turnCount): Promise<void> {
			await db
				.update(workers)
				.set({ messagesJson, turnCount, updatedAt: Date.now() })
				.where(eq(workers.id, id));
		},
		async markStarted(id): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "running", startedAt: now, updatedAt: now })
				.where(eq(workers.id, id));
		},
		async markIdle(id, messagesJson): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "idle", messagesJson, updatedAt: now })
				.where(eq(workers.id, id));
		},
		async markCompleted(id, messagesJson): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({
					status: "completed",
					messagesJson,
					completedAt: now,
					updatedAt: now,
					error: null,
				})
				.where(eq(workers.id, id));
		},
		async markFailed(id, error): Promise<void> {
			const now = Date.now();
			await db
				.update(workers)
				.set({ status: "failed", error, completedAt: now, updatedAt: now })
				.where(eq(workers.id, id));
		},
		async close(): Promise<void> {
			await sql.end({ timeout: 5 });
		},
	};
}

// ── Row casting ─────────────────────────────────────────────────────────

function castRow(row: {
	id: string;
	kind: string;
	lifetime: string;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo: string | null;
	name: string;
	initialPrompt: string | null;
	systemPrompt: string;
	modelProvider: string;
	modelId: string;
	status: string;
	messagesJson: string;
	containerName: string;
	createdAt: number;
	startedAt: number | null;
	updatedAt: number;
	completedAt: number | null;
	error: string | null;
	turnCount: number;
	sendPolicy: string;
}): WorkerRow {
	return {
		...row,
		kind: row.kind as WorkerKind,
		lifetime: row.lifetime as WorkerLifetime,
		status: row.status as WorkerStatus,
		sendPolicy: row.sendPolicy as SendPolicy,
	};
}
