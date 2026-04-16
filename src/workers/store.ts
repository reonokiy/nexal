/**
 * WorkerStore — Drizzle-backed persistence for sub-agent workers.
 *
 * Two driver modes:
 *   1. External Postgres via `drizzle-orm/bun-sql` (Bun.sql native)
 *      — set `url` to a postgres:// connection string.
 *   2. Embedded PGlite via `drizzle-orm/pglite` (WASM, zero-deps)
 *      — leave `url` empty; data persists to `~/.nexal/data/`.
 */
import { and, desc, eq, inArray, sql as dsql } from "drizzle-orm";
import { drizzle as drizzleBun } from "drizzle-orm/bun-sql";
import { drizzle as drizzlePglite } from "drizzle-orm/pglite";

import { createLog } from "../log.ts";
import * as schema from "./schema.ts";

const log = createLog("store");

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
	/**
	 * Postgres connection string, e.g. `postgres://user:pw@host:5432/db`.
	 * Leave empty to use embedded PGlite (data at `~/.nexal/data/`).
	 */
	url: string;
}

export async function createWorkerStore(cfg: WorkerStoreConfig): Promise<WorkerStore> {
	const { db, close } = await openDb(cfg);
	const { workers } = schema;

	return {
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

		async get(id: string): Promise<WorkerRow | null> {
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
				.where(and(eq(workers.id, id)));
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
			await close();
		},
	};
}

// ── Driver bootstrap ────────────────────────────────────────────────

type Db = ReturnType<typeof drizzleBun> | ReturnType<typeof drizzlePglite>;

async function openDb(cfg: WorkerStoreConfig): Promise<{ db: Db; close: () => Promise<void> }> {
	if (cfg.url) {
		// External Postgres via Bun.sql
		const sql = new (Bun as any).SQL(cfg.url);
		const db = drizzleBun(sql, { schema });
		return { db, close: () => sql.close() };
	}

	// Embedded PGlite — persist under ~/.nexal/data/
	const { mkdirSync } = await import("node:fs");
	const { join } = await import("node:path");
	const { homedir } = await import("node:os");
	const dataDir = join(homedir(), ".nexal", "data");
	mkdirSync(dataDir, { recursive: true });

	const { PGlite } = await import("@electric-sql/pglite");
	const client = new PGlite(dataDir);
	await client.waitReady;
	const db = drizzlePglite({ client, schema });

	// Auto-create the workers table if it doesn't exist.
	await db.execute(dsql`
		CREATE TABLE IF NOT EXISTS workers (
			id TEXT PRIMARY KEY,
			kind TEXT NOT NULL,
			lifetime TEXT NOT NULL,
			parent_session_key TEXT NOT NULL,
			source_channel TEXT NOT NULL,
			source_chat_id TEXT NOT NULL,
			source_reply_to TEXT,
			name TEXT NOT NULL,
			initial_prompt TEXT,
			system_prompt TEXT NOT NULL,
			model_provider TEXT NOT NULL,
			model_id TEXT NOT NULL,
			status TEXT NOT NULL,
			messages_json TEXT NOT NULL DEFAULT '[]',
			container_name TEXT NOT NULL,
			created_at BIGINT NOT NULL,
			started_at BIGINT,
			updated_at BIGINT NOT NULL,
			completed_at BIGINT,
			error TEXT,
			turn_count INTEGER NOT NULL DEFAULT 0,
			send_policy TEXT NOT NULL DEFAULT 'explicit'
		)
	`);
	await db.execute(dsql`CREATE INDEX IF NOT EXISTS workers_status_idx ON workers (status)`);
	await db.execute(dsql`CREATE INDEX IF NOT EXISTS workers_parent_idx ON workers (parent_session_key)`);

	log.success(`embedded PGlite database ready at ${dataDir}`);
	return { db, close: () => client.close() };
}

function castRow(row: typeof schema.workers.$inferSelect): WorkerRow {
	return {
		id: row.id,
		kind: row.kind as WorkerKind,
		lifetime: row.lifetime as WorkerLifetime,
		parentSessionKey: row.parentSessionKey,
		sourceChannel: row.sourceChannel,
		sourceChatId: row.sourceChatId,
		sourceReplyTo: row.sourceReplyTo,
		name: row.name,
		initialPrompt: row.initialPrompt,
		systemPrompt: row.systemPrompt,
		modelProvider: row.modelProvider,
		modelId: row.modelId,
		status: row.status as WorkerStatus,
		messagesJson: row.messagesJson,
		containerName: row.containerName,
		createdAt: row.createdAt,
		startedAt: row.startedAt,
		updatedAt: row.updatedAt,
		completedAt: row.completedAt,
		error: row.error,
		turnCount: row.turnCount,
		sendPolicy: row.sendPolicy as SendPolicy,
	};
}
