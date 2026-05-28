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
import { uuidv7 } from "uuidv7";

import { getDb } from "../db.ts";
import * as schema from "./schema.ts";
import { tapeRecord, type TapeEntry, type TapeEntryDraft, type TapeHandle, type TapeInfo, type TapeStore, type FileStore } from "@nexal/tape";

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

	async function appendTape(tape: TapeHandle, entry: TapeEntryDraft): Promise<TapeEntry>;
	async function appendTape(tape: TapeHandle, entries: TapeEntryDraft[]): Promise<TapeEntry[]>;
	async function appendTape(
		tape: TapeHandle,
		entryOrEntries: TapeEntryDraft | TapeEntryDraft[],
	): Promise<TapeEntry | TapeEntry[]> {
		const isBatch = Array.isArray(entryOrEntries);
		const entries = isBatch ? entryOrEntries : [entryOrEntries];
		if (entries.length === 0) return [];

		const prepared: TapeEntryDraft[] = [];
		for (const entry of entries) {
			const normalized = normalizeTapeEntryDraft(entry);
			prepared.push({
				...normalized,
				payload: fileStore
					? await offloadLargeBlobs(normalized.payload, fileStore, maxInline, db)
					: normalized.payload,
			});
		}

		const persisted = await db.transaction(async (tx) => {
			const tapeRecord = await requireTapeRecord(tx, tape);
			const firstId = tapeRecord.lastEntryId + 1;
			const lastId = tapeRecord.lastEntryId + prepared.length;

			await tx
				.update(tapes)
				.set({ lastEntryId: lastId })
				.where(eq(tapes.id, tapeRecord.id));

			const rows = await tx
				.insert(tapeEntries)
				.values(
					prepared.map((entry, index) => ({
						tapeId: tapeRecord.id,
						entryId: firstId + index,
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
					})),
				)
				.returning();

			return rows.map(rowToEntry);
		});

		return isBatch ? persisted : persisted[0]!;
	}

	return {
		async create(): Promise<TapeHandle> {
			const row = await createTapeRecord(db);
			return { tapeId: row.id };
		},

		async listTapes(): Promise<TapeInfo[]> {
			const rows = await db.select().from(tapes).orderBy(tapes.id);
			return Promise.all(rows.map((row) => infoForTapeRecord(row)));
		},

		async read(tape: TapeHandle): Promise<TapeEntry[]> {
			const tapeRecord = await findTapeRecordById(db, tape.tapeId);
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

		async readPage(
			tape: TapeHandle,
			options: { offset: number; limit: number },
		): Promise<TapeEntry[]> {
			const tapeRecord = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRecord) return [];
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(eq(tapeEntries.tapeId, tapeRecord.id))
				.orderBy(tapeEntries.entryId)
				.limit(options.limit)
				.offset(options.offset);
			const entries = rows.map(rowToEntry);
			if (!fileStore) return entries;
			for (const e of entries) {
				e.payload = await hydrateFileRefs(e.payload, fileStore);
			}
			return entries;
		},

		append: appendTape,

		async reset(tapeRef: TapeHandle): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRecord = await findTapeRecordById(tx, tapeRef.tapeId);
				if (tapeRecord) {
					await tx.delete(tapeEntries).where(eq(tapeEntries.tapeId, tapeRecord.id));
					await tx
						.update(tapes)
						.set({ lastEntryId: 0 })
						.where(eq(tapes.id, tapeRecord.id));
				}
			});
		},

		async delete(tapeRef: TapeHandle): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRecord = await findTapeRecordById(tx, tapeRef.tapeId);
				if (tapeRecord) await tx.delete(tapes).where(eq(tapes.id, tapeRecord.id));
			});
		},

		async info(tape: TapeHandle): Promise<TapeInfo> {
			const tapeRecord = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRecord) {
				return {
					id: tape.tapeId,
					entries: 0,
					anchors: 0,
					lastAnchor: null,
					entriesSinceLastAnchor: 0,
					lastTokenUsage: null,
				};
			}
			return infoForTapeRecord(tapeRecord);
		},

		async handoff(tape: TapeHandle, name: string, state?: Record<string, unknown>): Promise<void> {
			await appendTape(tape, tapeRecord.anchor(name, state));
		},

		async search(tape: TapeHandle, query: string, limit = 20): Promise<TapeEntry[]> {
			const tapeRecord = await findTapeRecordById(db, tape.tapeId);
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

	async function infoForTapeRecord(tapeRecord: schema.TapeRow): Promise<TapeInfo> {
		const [statsRow] = await db
			.select({
				entries: sql<number>`count(*)::int`,
				anchors: sql<number>`count(*) filter (where ${tapeEntries.kind} = 'anchor')::int`,
			})
			.from(tapeEntries)
			.where(eq(tapeEntries.tapeId, tapeRecord.id));
		const [lastAnchorRow] = await db
			.select({
				entryId: tapeEntries.entryId,
				payload: tapeEntries.payload,
			})
			.from(tapeEntries)
			.where(and(eq(tapeEntries.tapeId, tapeRecord.id), eq(tapeEntries.kind, "anchor")))
			.orderBy(desc(tapeEntries.entryId))
			.limit(1);
		const [lastRunRow] = await db
			.select({ payload: tapeEntries.payload })
			.from(tapeEntries)
			.where(
				and(
					eq(tapeEntries.tapeId, tapeRecord.id),
					eq(tapeEntries.kind, "event"),
					sql`${tapeEntries.payload}->>'name' = 'run'`,
				),
			)
			.orderBy(desc(tapeEntries.entryId))
			.limit(1);
		const entries = statsRow?.entries ?? 0;
		const anchors = statsRow?.anchors ?? 0;
		const lastAnchorId = lastAnchorRow?.entryId ?? null;
		const entriesSinceLastAnchor =
			lastAnchorId !== null ? entries - lastAnchorId : entries;
		const lastTokenUsage = (lastRunRow?.payload.data as any)?.usage?.total_tokens;
		return {
			id: tapeRecord.id,
			entries,
			anchors,
			lastAnchor: lastAnchorRow ? String(lastAnchorRow.payload.name ?? null) : null,
			entriesSinceLastAnchor,
			lastTokenUsage: typeof lastTokenUsage === "number" ? lastTokenUsage : null,
		};
	}

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

export async function getOrCreateSessionTapeRef(sessionKey: string): Promise<TapeHandle> {
	const db = getDb();
	const existing = await db
		.select()
		.from(schema.sessionTapes)
		.where(eq(schema.sessionTapes.sessionKey, sessionKey));
	if (existing[0]) return { tapeId: existing[0].tapeId };

	const now = Date.now();
	const row = await db.transaction(async (tx) => {
		const created = await createTapeRecord(tx);
		const [mapping] = await tx
			.insert(schema.sessionTapes)
			.values({
				sessionKey,
				tapeId: created.id,
				createdAt: now,
				updatedAt: now,
			})
			.onConflictDoUpdate({
				target: schema.sessionTapes.sessionKey,
				set: { updatedAt: now },
			})
			.returning();
		return mapping ?? { tapeId: created.id };
	});
	return { tapeId: row.tapeId };
}

export async function getSessionTapeRef(sessionKey: string): Promise<TapeHandle | null> {
	const db = getDb();
	const existing = await db
		.select()
		.from(schema.sessionTapes)
		.where(eq(schema.sessionTapes.sessionKey, sessionKey));
	return existing[0] ? { tapeId: existing[0].tapeId } : null;
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
			const url = await store.getUrl(block.fileRef);
			if (url) {
				next.push({ ...block, url });
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

async function findTapeRecordById(
	db: any,
	id: string,
): Promise<schema.TapeRow | null> {
	const rows = await db
		.select()
		.from(schema.tapes)
		.where(eq(schema.tapes.id, id));
	return rows[0] ?? null;
}

async function requireTapeRecord(
	tx: any,
	ref: TapeHandle,
): Promise<schema.TapeRow> {
	const existing = await findTapeRecordById(tx, ref.tapeId);
	if (!existing) throw new Error(`Tape not found: ${ref.tapeId}`);
	return existing;
}

async function createTapeRecord(tx: any): Promise<schema.TapeRow> {
	const [inserted] = await tx
		.insert(schema.tapes)
		.values({
			id: uuidv7(),
			lastEntryId: 0,
			createdAt: Date.now(),
		})
		.returning();
	if (!inserted) throw new Error("Failed to create tape");
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

export function normalizeTapeEntryDraft(entry: TapeEntryDraft): TapeEntryDraft {
	if (entry.kind === "tool_call") return normalizeLegacyToolCallEntry(entry);
	if (entry.kind !== "message") return entry;

	const content = entry.payload.content;
	if (!Array.isArray(content)) return entry;

	let changed = false;
	const normalizedContent = content.map((block) => {
		if (!block || typeof block !== "object" || Array.isArray(block)) return block;
		const record = block as Record<string, unknown>;
		if (record.type !== "tool_call") return block;
		changed = true;
		const {
			type: _type,
			id,
			name,
			toolCallId,
			toolName,
			arguments: args,
			args: legacyArgs,
			input,
			...rest
		} = record;
		return {
			...rest,
			type: "toolCall",
			id: id ?? toolCallId ?? "",
			name: name ?? toolName ?? "tool",
			arguments: args ?? legacyArgs ?? input ?? {},
		};
	});
	if (!changed) return entry;
	return {
		...entry,
		payload: {
			...entry.payload,
			content: normalizedContent,
		},
	};
}

function normalizeLegacyToolCallEntry(entry: TapeEntryDraft): TapeEntryDraft {
	const payload = entry.payload;
	const {
		toolCallId,
		toolName,
		id,
		name,
		arguments: args,
		args: legacyArgs,
		input,
		thoughtSignature,
		role: _role,
		content: _content,
		...rest
	} = payload;
	const callId = String(toolCallId ?? id ?? "");
	const callName = String(toolName ?? name ?? "tool");
	const parsedDate = Date.parse(entry.date);
	const timestamp = Number(payload.timestamp ?? (Number.isFinite(parsedDate) ? parsedDate : Date.now()));
	return {
		...entry,
		kind: "message",
		payload: {
			...rest,
			role: "assistant",
			content: [{
				type: "toolCall",
				id: callId,
				name: callName,
				arguments: args ?? legacyArgs ?? input ?? {},
				...(thoughtSignature ? { thoughtSignature } : {}),
			}],
			timestamp,
		},
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
