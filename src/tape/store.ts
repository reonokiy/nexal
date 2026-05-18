/**
 * TapeStore — Postgres-backed append-only tape storage.
 *
 * Ports the SQLAlchemy backend from bub-tapestore-sqlalchemy to
 * Drizzle ORM + Postgres. Every operation is a single transaction
 * where needed (append does read-then-write inside a transaction
 * to allocate monotonic entry_ids safely).
 *
 * Invariants:
 *   1. History is append-only — entries are never overwritten.
 *   2. Derivatives never replace original facts.
 *   3. Context is constructed, not inherited wholesale.
 */
import { eq, and, desc, sql } from "drizzle-orm";
import { createHash } from "node:crypto";

import { getDb } from "../db.ts";
import * as schema from "./schema.ts";
import type { TapeEntry, TapeInfo } from "./types.ts";

export interface TapeStore {
	/** Return all tape names ordered alphabetically. */
	listTapes(): Promise<string[]>;
	/** Read every entry for a tape, ordered by entry_id. */
	read(tape: string): Promise<TapeEntry[]>;
	/** Append one entry to the tail of a tape (allocates entry_id). */
	append(tape: string, entry: Omit<TapeEntry, "id">): Promise<void>;
	/** Delete all entries for a tape (hard reset). */
	reset(tape: string): Promise<void>;
	/** Runtime summary (entry count, anchors, last anchor, …). */
	info(tape: string): Promise<TapeInfo>;
	/** Write a new anchor entry (handoff). */
	handoff(tape: string, name: string, state?: Record<string, unknown>): Promise<void>;
	/** Search entries by fuzzy text match in payload (simple LIKE). */
	search(tape: string, query: string, limit?: number): Promise<TapeEntry[]>;
}

export function createTapeStore(): TapeStore {
	const db = getDb();
	const { tapes, tapeEntries } = schema;

	return {
		async listTapes(): Promise<string[]> {
			const rows = await db.select({ name: tapes.name }).from(tapes).orderBy(tapes.name);
			return rows.map((r) => r.name);
		},

		async read(tape: string): Promise<TapeEntry[]> {
			const tapeRecord = await findTapeRecord(db, tape);
			if (!tapeRecord) return [];
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(eq(tapeEntries.tapeId, tapeRecord.id))
				.orderBy(tapeEntries.entryId);
			return rows.map(rowToEntry);
		},

		async append(tape: string, entry: Omit<TapeEntry, "id">): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRecord = await loadOrCreateTape(tx, tape);
				const nextId = tapeRecord.lastEntryId + 1;

				await tx
					.update(tapes)
					.set({ lastEntryId: nextId })
					.where(eq(tapes.id, tapeRecord.id));

				await tx.insert(tapeEntries).values({
					tapeId: tapeRecord.id,
					entryId: nextId,
					kind: entry.kind,
					anchorName: entry.kind === "anchor" ? String(entry.payload.name ?? "") : null,
					anchorNameKey:
						entry.kind === "anchor" && entry.payload.name
							? sha256Key(String(entry.payload.name))
								: null,
					payload: entry.payload,
					meta: entry.meta,
					entryDate: entry.date,
					createdAt: Date.now(),
				});
			});
		},

		async reset(tape: string): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRecord = await findTapeRecord(tx, tape);
				if (tapeRecord) {
					await tx.delete(tapeEntries).where(eq(tapeEntries.tapeId, tapeRecord.id));
					await tx
						.update(tapes)
						.set({ lastEntryId: 0 })
						.where(eq(tapes.id, tapeRecord.id));
				}
			});
		},

		async info(tape: string): Promise<TapeInfo> {
			const tapeRecord = await findTapeRecord(db, tape);
			if (!tapeRecord) {
				return {
					name: tape,
					entries: 0,
					anchors: 0,
					lastAnchor: null,
					entriesSinceLastAnchor: 0,
					lastTokenUsage: null,
				};
			}
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(eq(tapeEntries.tapeId, tapeRecord.id))
				.orderBy(tapeEntries.entryId);
			const entries = rows.map(rowToEntry);
			const anchors = entries.filter((e) => e.kind === "anchor");
			const lastAnchor = anchors.length > 0 ? anchors[anchors.length - 1]! : null;
			let entriesSinceLastAnchor = 0;
			if (lastAnchor) {
				const lastAnchorIdx = entries.findIndex((e) => e.id === lastAnchor.id);
				entriesSinceLastAnchor = entries.length - lastAnchorIdx - 1;
			} else {
				entriesSinceLastAnchor = entries.length;
			}
			let lastTokenUsage: number | null = null;
			for (let i = entries.length - 1; i >= 0; i--) {
				const e = entries[i]!;
				if (e.kind === "event" && e.payload.name === "run") {
					const usage = (e.payload.data as any)?.usage?.total_tokens;
					if (typeof usage === "number") {
						lastTokenUsage = usage;
						break;
					}
				}
			}
			return {
				name: tape,
				entries: entries.length,
				anchors: anchors.length,
				lastAnchor: lastAnchor ? String(lastAnchor.payload.name ?? null) : null,
				entriesSinceLastAnchor,
				lastTokenUsage,
			};
		},

		async handoff(tape: string, name: string, state?: Record<string, unknown>): Promise<void> {
			const payload: Record<string, unknown> = { name };
			if (state) payload.state = state;
			await this.append(tape, {
				kind: "anchor",
				payload,
				meta: {},
				date: new Date().toISOString(),
			});
		},

		async search(tape: string, query: string, limit = 20): Promise<TapeEntry[]> {
			const tapeRecord = await findTapeRecord(db, tape);
			if (!tapeRecord) return [];
			const pattern = `%${escapeLike(query)}%`;
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(
					and(
						eq(tapeEntries.tapeId, tapeRecord.id),
						sql`${tapeEntries.payload}::text LIKE ${pattern}`,
					),
				)
				.orderBy(desc(tapeEntries.entryId))
				.limit(limit);
			return rows.map(rowToEntry);
		},
	};
}

// ── helpers ──────────────────────────────────────────────────────────

async function findTapeRecord(
	db: any,
	name: string,
): Promise<schema.TapeRow | null> {
	const rows = await db
		.select()
		.from(schema.tapes)
		.where(eq(schema.tapes.nameKey, sha256Key(name)));
	return rows[0] ?? null;
}

async function loadOrCreateTape(
	tx: any,
	name: string,
): Promise<schema.TapeRow> {
	const existing = await findTapeRecord(tx, name);
	if (existing) return existing;
	const [inserted] = await tx
		.insert(schema.tapes)
		.values({
			name,
			nameKey: sha256Key(name),
			lastEntryId: 0,
			createdAt: Date.now(),
		})
		.returning();
	if (!inserted) throw new Error(`Failed to create tape: ${name}`);
	return inserted;
}

function rowToEntry(row: schema.TapeEntryRow): TapeEntry {
	return {
		id: row.entryId,
		kind: row.kind as TapeEntry["kind"],
		payload: (row.payload as Record<string, unknown>) ?? {},
		meta: (row.meta as Record<string, unknown>) ?? {},
		date: row.entryDate,
	};
}

function sha256Key(value: string): string {
	return createHash("sha256").update(value, "utf-8").digest("hex");
}

function escapeLike(value: string): string {
	return value.replace(/[\\%_]/g, "\\$&");
}
