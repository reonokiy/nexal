import { createHash } from "node:crypto";
import { eq, and, desc, sql } from "drizzle-orm";
import { Tape as BaseTape } from "@nexal/tape";
import type {
	FileRef,
	FileStore,
	TapeEntry,
	TapeEntryDraft,
	TapeFactoryOptions,
	TapeHandle,
	TapeInfo,
	TapeStore,
} from "@nexal/tape";
import { uuidv7 } from "uuidv7";
import type { StorageConfig } from "../config.ts";
import { getDb } from "../db.ts";
import * as schema from "./schema.ts";

export interface NexalSessionContext {
	channel: string;
	chatId: string;
	sessionKey: string;
	streaming?: boolean;
	debounce?: unknown;
}

export interface NexalWorkerContext {
	id: string;
	name: string;
	kind: string;
	lifetime: string;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
	initialPrompt?: string | null;
	sendPolicy: string;
	status: string;
	containerName?: string | null;
	sandboxKey?: string;
	sandboxed?: boolean;
	resumed?: boolean;
}

/** Nexal-specific semantics layered on top of the generic tape package. */
export class NexalTape extends BaseTape {
	static createFileStore(cfg: StorageConfig): FileStore {
		return new NexalS3FileStore(cfg);
	}

	static createTapeStore(options: TapeStoreOptions = {}): TapeStore {
		return createTapeStore(options);
	}

	static async create(store: TapeStore, maxContext?: number): Promise<NexalTape>;
	static async create(store: TapeStore, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async create(store: TapeStore, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		return new NexalTape({ store, ref: await store.create(), ...factoryOptions(optionsOrMaxContext) });
	}

	static async load(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<NexalTape>;
	static async load(store: TapeStore, ref: TapeHandle, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async load(store: TapeStore, ref: TapeHandle, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		const tape = new NexalTape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
		await tape.entries();
		return tape;
	}

	static async loadOrCreate(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<NexalTape>;
	static async loadOrCreate(store: TapeStore, ref: TapeHandle, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async loadOrCreate(store: TapeStore, ref: TapeHandle, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		return new NexalTape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
	}

	async setSessionContext(context: NexalSessionContext): Promise<Array<TapeEntry | null>> {
		const metadata = {
			channel: context.channel,
			chatId: context.chatId,
			sessionKey: context.sessionKey,
		};
		const entries: Array<TapeEntry | null> = [];
		entries.push(await this.setContext(metadata, {
			scope: "session",
			ifChanged: true,
		}));
		entries.push(await this.setPolicy({
			streaming: context.streaming ?? false,
			debounce: context.debounce ?? null,
		}, {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setRuntime({
			type: "llm-session",
		}, {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setStatus("running", {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		return entries;
	}

	async setWorkerContext(context: NexalWorkerContext): Promise<Array<TapeEntry | null>> {
		const metadata = {
			id: context.id,
			name: context.name,
			kind: context.kind,
			lifetime: context.lifetime,
			parentSessionKey: context.parentSessionKey,
			sourceChannel: context.sourceChannel,
			sourceChatId: context.sourceChatId,
			sourceReplyTo: context.sourceReplyTo,
			sendPolicy: context.sendPolicy,
			initialPrompt: context.initialPrompt,
			containerName: context.containerName,
		};
		const entries: Array<TapeEntry | null> = [];
		entries.push(await this.setContext({
			id: context.id,
			name: context.name,
			kind: context.kind,
			parentSessionKey: context.parentSessionKey,
			sourceChannel: context.sourceChannel,
			sourceChatId: context.sourceChatId,
			sourceReplyTo: context.sourceReplyTo,
			initialPrompt: context.initialPrompt,
		}, {
			scope: "worker",
			ifChanged: true,
		}));
		entries.push(await this.setPolicy({
			lifetime: context.lifetime,
			sendPolicy: context.sendPolicy,
		}, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setRuntime({
			type: "agent-runner",
			sandboxKey: context.sandboxKey,
			containerName: context.containerName,
			sandboxed: context.sandboxed ?? context.kind === "executor",
			resumed: context.resumed ?? false,
		}, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setStatus(context.status, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		return entries;
	}
}

export function createFileStore(cfg: StorageConfig): FileStore {
	return NexalTape.createFileStore(cfg);
}

class NexalS3FileStore implements FileStore {
	private readonly client: any; // Bun.S3Client

	constructor(cfg: StorageConfig) {
		if (cfg.provider !== "s3") {
			throw new Error(`Unsupported storage provider: ${cfg.provider}`);
		}
		this.client = new (Bun as any).S3Client({
			accessKeyId: cfg.s3AccessKey,
			secretAccessKey: cfg.s3SecretKey,
			endpoint: cfg.s3Endpoint,
			bucket: cfg.s3Bucket,
			region: cfg.s3Region,
		});
	}

	async upload(
		data: Uint8Array | Buffer,
		mimeType: string,
		filename: string,
	): Promise<FileRef> {
		const hash = sha256Hex(data);
		const file = this.client.file(hashPath(hash));
		await file.write(data, { type: mimeType });
		return {
			fileHash: hash,
			mimeType,
			filename,
			sizeBytes: data.byteLength,
			url: await file.presignedUrl({ expiresIn: 3600 }),
		};
	}

	async download(fileHash: string): Promise<Uint8Array | null> {
		try {
			const file = this.client.file(hashPath(fileHash));
			return new Uint8Array(await file.arrayBuffer());
		} catch {
			return null;
		}
	}

	async getUrl(fileHash: string): Promise<string | null> {
		try {
			const file = this.client.file(hashPath(fileHash));
			return await file.presignedUrl({ expiresIn: 3600 });
		} catch {
			return null;
		}
	}
}

function sha256Hex(data: Uint8Array | Buffer): string {
	return createHash("sha256").update(data).digest("hex");
}

function hashPath(hash: string): string {
	return `${hash.slice(0, 2)}/${hash.slice(2, 4)}/${hash}`;
}

export interface TapeStoreOptions {
	/** Off-load oversized binary blocks to this store. Optional. */
	fileStore?: FileStore;
	/** Inline cutoff (bytes). Default 8 KiB. */
	maxInlineSize?: number;
}

const DEFAULT_MAX_INLINE = 8_192;

/** Create Nexal's Postgres-backed tape store. */
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
			const tapeRow = await requireTapeRecord(tx, tape);
			const firstId = tapeRow.lastEntryId + 1;
			const lastId = tapeRow.lastEntryId + prepared.length;

			await tx
				.update(tapes)
				.set({ lastEntryId: lastId })
				.where(eq(tapes.id, tapeRow.id));

			const rows = await tx
				.insert(tapeEntries)
				.values(
					prepared.map((entry, index) => ({
						tapeId: tapeRow.id,
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
			const tapeRow = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRow) return [];
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(eq(tapeEntries.tapeId, tapeRow.id))
				.orderBy(tapeEntries.entryId);
			return hydrateEntries(rows.map(rowToEntry), fileStore);
		},

		async readPage(
			tape: TapeHandle,
			options: { offset: number; limit: number },
		): Promise<TapeEntry[]> {
			const tapeRow = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRow) return [];
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(eq(tapeEntries.tapeId, tapeRow.id))
				.orderBy(tapeEntries.entryId)
				.limit(options.limit)
				.offset(options.offset);
			return hydrateEntries(rows.map(rowToEntry), fileStore);
		},

		append: appendTape,

		async reset(tapeRef: TapeHandle): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRow = await findTapeRecordById(tx, tapeRef.tapeId);
				if (tapeRow) {
					await tx.delete(tapeEntries).where(eq(tapeEntries.tapeId, tapeRow.id));
					await tx
						.update(tapes)
						.set({ lastEntryId: 0 })
						.where(eq(tapes.id, tapeRow.id));
				}
			});
		},

		async delete(tapeRef: TapeHandle): Promise<void> {
			await db.transaction(async (tx) => {
				const tapeRow = await findTapeRecordById(tx, tapeRef.tapeId);
				if (tapeRow) await tx.delete(tapes).where(eq(tapes.id, tapeRow.id));
			});
		},

		async info(tape: TapeHandle): Promise<TapeInfo> {
			const tapeRow = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRow) {
				return {
					id: tape.tapeId,
					entries: 0,
					anchors: 0,
					lastAnchor: null,
					entriesSinceLastAnchor: 0,
					lastTokenUsage: null,
				};
			}
			return infoForTapeRecord(tapeRow);
		},

		async handoff(tape: TapeHandle, name: string, state?: Record<string, unknown>): Promise<void> {
			await appendTape(tape, anchorDraft(name, state));
		},

		async search(tape: TapeHandle, query: string, limit = 20): Promise<TapeEntry[]> {
			const tapeRow = await findTapeRecordById(db, tape.tapeId);
			if (!tapeRow) return [];
			const pattern = `%${escapeLike(query)}%`;
			const rows = await db
				.select()
				.from(tapeEntries)
				.where(and(
					eq(tapeEntries.tapeId, tapeRow.id),
					sql`${tapeEntries.payload}::text LIKE ${pattern}`,
				))
				.orderBy(desc(tapeEntries.entryId))
				.limit(limit);
			return hydrateEntries(rows.map(rowToEntry), fileStore);
		},
	};

	async function infoForTapeRecord(tapeRow: schema.TapeRow): Promise<TapeInfo> {
		const [statsRow] = await db
			.select({
				entries: sql<number>`count(*)::int`,
				anchors: sql<number>`count(*) filter (where ${tapeEntries.kind} = 'anchor')::int`,
			})
			.from(tapeEntries)
			.where(eq(tapeEntries.tapeId, tapeRow.id));
		const [lastAnchorRow] = await db
			.select({ entryId: tapeEntries.entryId, payload: tapeEntries.payload })
			.from(tapeEntries)
			.where(and(eq(tapeEntries.tapeId, tapeRow.id), eq(tapeEntries.kind, "anchor")))
			.orderBy(desc(tapeEntries.entryId))
			.limit(1);
		const [lastRunRow] = await db
			.select({ payload: tapeEntries.payload })
			.from(tapeEntries)
			.where(and(
				eq(tapeEntries.tapeId, tapeRow.id),
				eq(tapeEntries.kind, "event"),
				sql`${tapeEntries.payload}->>'name' = 'run'`,
			))
			.orderBy(desc(tapeEntries.entryId))
			.limit(1);
		const entries = statsRow?.entries ?? 0;
		const anchors = statsRow?.anchors ?? 0;
		const lastAnchorId = lastAnchorRow?.entryId ?? null;
		const entriesSinceLastAnchor = lastAnchorId !== null ? entries - lastAnchorId : entries;
		const lastTokenUsage = (lastRunRow?.payload.data as any)?.usage?.total_tokens;
		return {
			id: tapeRow.id,
			entries,
			anchors,
			lastAnchor: typeof lastAnchorRow?.payload.name === "string" ? lastAnchorRow.payload.name : null,
			entriesSinceLastAnchor,
			lastTokenUsage: typeof lastTokenUsage === "number" ? lastTokenUsage : null,
		};
	}

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
		return mutated ? { ...payload, content: next } : payload;
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
			.values({ sessionKey, tapeId: created.id, createdAt: now, updatedAt: now })
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

async function hydrateEntries(entries: TapeEntry[], store: FileStore | undefined): Promise<TapeEntry[]> {
	if (!store) return entries;
	return Promise.all(entries.map(async (entry) => ({
		...entry,
		payload: await hydrateFileRefs(entry.payload, store),
	})));
}

async function hydrateFileRefs(payload: Record<string, unknown>, store: FileStore): Promise<Record<string, unknown>> {
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
	return mutated ? { ...payload, content: next } : payload;
}

function isObject(v: unknown): v is Record<string, unknown> {
	return typeof v === "object" && v !== null && !Array.isArray(v);
}

async function findTapeRecordById(db: any, id: string): Promise<schema.TapeRow | null> {
	const rows = await db.select().from(schema.tapes).where(eq(schema.tapes.id, id));
	return rows[0] ?? null;
}

async function requireTapeRecord(tx: any, ref: TapeHandle): Promise<schema.TapeRow> {
	const existing = await findTapeRecordById(tx, ref.tapeId);
	if (!existing) throw new Error(`Tape not found: ${ref.tapeId}`);
	return existing;
}

async function createTapeRecord(tx: any): Promise<schema.TapeRow> {
	const [inserted] = await tx
		.insert(schema.tapes)
		.values({ id: uuidv7(), lastEntryId: 0, createdAt: Date.now() })
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

function anchorDraft(name: string, state?: Record<string, unknown>): TapeEntryDraft {
	const payload: Record<string, unknown> = { name };
	if (state) payload.state = state;
	return { kind: "anchor", payload, meta: {}, date: new Date().toISOString() };
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
	return { ...entry, payload: { ...entry.payload, content: normalizedContent } };
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

function factoryOptions(optionsOrMaxContext?: number | TapeFactoryOptions): TapeFactoryOptions {
	return typeof optionsOrMaxContext === "number"
		? { maxContext: optionsOrMaxContext }
		: optionsOrMaxContext ?? {};
}
