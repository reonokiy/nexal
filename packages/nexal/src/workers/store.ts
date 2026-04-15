/**
 * WorkerStore — Drizzle-backed persistence for sub-agent workers.
 *
 * Postgres-only via `drizzle-orm/bun-sql` (uses Bun's native
 * `Bun.sql` driver, no extra npm deps). Earlier dual-driver design
 * was dropped after confirming Drizzle has no plan to support
 * dialect-agnostic schemas — see https://github.com/drizzle-team/drizzle-orm/discussions/2469.
 */
import { and, desc, eq, inArray } from "drizzle-orm";
import { drizzle } from "drizzle-orm/bun-sql";

import * as schema from "./schema.ts";

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
	/** Postgres connection string, e.g. `postgres://user:pw@host:5432/db`. */
	url: string;
}

export async function createWorkerStore(cfg: WorkerStoreConfig): Promise<WorkerStore> {
	if (!cfg.url) throw new Error("workers.url (postgres connection string) required");
	const sql = new (Bun as any).SQL(cfg.url);
	const db = drizzle(sql, { schema });
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
			await sql.close();
		},
	};
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
