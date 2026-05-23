/**
 * TapeStore — Postgres-backed append-only tape storage.
 *
 * Drizzle ORM + Postgres implementation of the TapeStore interface.
 * Every operation is a single transaction where needed (append does
 * read-then-write inside a transaction to allocate monotonic entry_ids).
 *
 * Optional `fileStore`: image blocks whose base64 payload exceeds
 * `maxInlineSize` bytes are off-loaded to the FileStore (S3 or local)
 * and replaced by a `{type:"image", fileRef:<sha256>, mimeType}`
 * placeholder in the entry payload. On `read`, those placeholders are
 * hydrated back to `{type:"image", data:<base64>, mimeType}` so the
 * caller never has to know the file lives outside the DB.
 */
import { eq, and, desc, sql } from "drizzle-orm";
import { createHash } from "node:crypto";

import { getDb } from "../db.ts";
import * as schema from "./schema.ts";
import type { TapeEntry, TapeInfo, TapeStore, FileStore } from "@nexal/tape";

export interface TapeStoreOptions {
	/** Off-load oversized binary blocks to this store. Optional. */
	fileStore?: FileStore;
	/** Inline cutoff (bytes). Default 8 KiB. */
	maxInlineSize?: number;
}

const DEFAULT_MAX_INLINE = 8_192;

export function createTapeStore(opts: TapeStoreOptions = {}): TapeStore {
	const db = getDb();
	const { tapes, tapeEntries, tapeFiles } = schema;
	const fileStore = opts.fileStore;
	const maxInline = opts.maxInlineSize ?? DEFAULT_MAX_INLINE;

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
			const entries = rows.map(rowToEntry);
			if (!fileStore) return entries;
			for (const e of entries) {
				e.payload = await hydrateFileRefs(e.payload, fileStore);
			}
			return entries;
		},

		async append(tape: string, entry: Omit<TapeEntry, "id">): Promise<void> {
			const payload = fileStore
				? await offloadLargeBlobs(entry.payload, fileStore, maxInline, db)
				: entry.payload;

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
					payload,
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
			const lastAnchor = entries.findLast((e) => e.kind === "anchor") ?? null;
			const entriesSinceLastAnchor = lastAnchor
				? entries.length - entries.findLastIndex((e) => e.id === lastAnchor.id) - 1
				: entries.length;
			const lastTokenUsage = findLastTokenUsage(entries);
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

	// ── inline helpers (capture `tapeFiles`, `db`) ─────────────────────

	async function offloadLargeBlobs(
		payload: Record<string, unknown>,
		store: FileStore,
		threshold: number,
		dbConn: ReturnType<typeof getDb>,
	): Promise<Record<string, unknown>> {
		const content = payload.content;
		if (!Array.isArray(content)) return payload;
		let mutated = false;
		const next: unknown[] = [];
		for (const block of content) {
			const out = await maybeOffloadBlock(block, store, threshold, dbConn);
			if (out !== block) mutated = true;
			next.push(out);
		}
		if (!mutated) return payload;
		return { ...payload, content: next };
	}

	async function maybeOffloadBlock(
		block: unknown,
		store: FileStore,
		threshold: number,
		dbConn: ReturnType<typeof getDb>,
	): Promise<unknown> {
		if (!isObject(block)) return block;
		if (block.type !== "image") return block;
		const data = block.data;
		if (typeof data !== "string") return block;
		if (data.length * 0.75 < threshold) return block;
		const mimeType = typeof block.mimeType === "string" ? block.mimeType : "application/octet-stream";
		const bytes = Buffer.from(data, "base64");
		const ref = await store.upload(bytes, mimeType, "tape-image");
		await dbConn
			.insert(tapeFiles)
			.values({
				fileHash: ref.fileHash,
				sizeBytes: ref.sizeBytes,
				mimeType: ref.mimeType,
				createdAt: Date.now(),
			})
			.onConflictDoNothing({ target: tapeFiles.fileHash });
		return { type: "image", fileRef: ref.fileHash, mimeType };
	}
}

// ── helpers ──────────────────────────────────────────────────────────

async function hydrateFileRefs(
	payload: Record<string, unknown>,
	store: FileStore,
): Promise<Record<string, unknown>> {
	const content = payload.content;
	if (!Array.isArray(content)) return payload;
	let mutated = false;
	const next: unknown[] = [];
	for (const block of content) {
		if (isObject(block) && block.type === "image" && typeof block.fileRef === "string") {
			const bytes = await store.download(block.fileRef);
			if (bytes) {
				next.push({
					type: "image",
					data: Buffer.from(bytes).toString("base64"),
					mimeType: typeof block.mimeType === "string" ? block.mimeType : "application/octet-stream",
				});
				mutated = true;
				continue;
			}
		}
		next.push(block);
	}
	if (!mutated) return payload;
	return { ...payload, content: next };
}

function isObject(v: unknown): v is Record<string, unknown> {
	return typeof v === "object" && v !== null && !Array.isArray(v);
}

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
		payload: row.payload ?? {},
		meta: row.meta ?? {},
		date: row.entryDate,
	};
}

function sha256Key(value: string): string {
	return createHash("sha256").update(value, "utf-8").digest("hex");
}

function escapeLike(value: string): string {
	return value.replace(/[\\%_]/g, "\\$&");
}

function findLastTokenUsage(entries: TapeEntry[]): number | null {
	for (let i = entries.length - 1; i >= 0; i--) {
		const e = entries[i]!;
		if (e.kind === "event" && e.payload.name === "run") {
			const usage = (e.payload.data as any)?.usage?.total_tokens;
			if (typeof usage === "number") return usage;
		}
	}
	return null;
}
