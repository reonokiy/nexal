/**
 * Drizzle schema for the tape.systems model.
 *
 * Tables:
 *   tapes       — one row per tape (session/workspace context)
 *   tape_entries — append-only facts
 *   tape_files   — external binary file metadata (hash-deduplicated)
 */
import {
	bigint,
	index,
	integer,
	jsonb,
	pgTable,
	primaryKey,
	serial,
	text,
	uuid,
	varchar,
} from "drizzle-orm/pg-core";

// ── tapes ────────────────────────────────────────────────────────────

export const tapes = pgTable("tapes", {
	id: uuid("id").primaryKey(),
	/** Highest entry_id written to this tape. */
	lastEntryId: integer("last_entry_id").notNull().default(0),
	createdAt: bigint("created_at", { mode: "number" }).notNull(),
});

export const sessionTapes = pgTable("session_tapes", {
	sessionKey: text("session_key").primaryKey(),
	tapeId: uuid("tape_id")
		.notNull()
		.references(() => tapes.id, { onDelete: "cascade" }),
	createdAt: bigint("created_at", { mode: "number" }).notNull(),
	updatedAt: bigint("updated_at", { mode: "number" }).notNull(),
});

export type SessionTapeRow = typeof sessionTapes.$inferSelect;

export type TapeRow = typeof tapes.$inferSelect;
export type TapeInsert = typeof tapes.$inferInsert;

// ── tape_entries ─────────────────────────────────────────────────────

export const tapeEntries = pgTable(
	"tape_entries",
	{
		tapeId: uuid("tape_id")
			.notNull()
			.references(() => tapes.id, { onDelete: "cascade" }),
		entryId: integer("entry_id").notNull(),
		kind: text("kind").notNull(),
		/** Only set for anchor entries (e.g. "session/start"). */
		anchorName: text("anchor_name"),
		anchorNameKey: varchar("anchor_name_key", { length: 64 }),
		payload: jsonb("payload").notNull().$type<Record<string, unknown>>(),
		meta: jsonb("meta")
			.notNull()
			.default({})
			.$type<Record<string, unknown>>(),
		/** ISO-8601 string (mirrors upstream tape.systems). */
		entryDate: text("entry_date").notNull(),
		createdAt: bigint("created_at", { mode: "number" }).notNull(),
	},
	(t) => ({
		pk: primaryKey({ columns: [t.tapeId, t.entryId] }),
		kindIdx: index("idx_tape_entries_kind").on(t.tapeId, t.kind, t.entryId),
		anchorIdx: index("idx_tape_entries_anchor").on(
			t.tapeId,
			t.anchorNameKey,
			t.entryId,
		),
	}),
);

export type TapeEntryRow = typeof tapeEntries.$inferSelect;
export type TapeEntryInsert = typeof tapeEntries.$inferInsert;

// ── tape_files ───────────────────────────────────────────────────────
//
// Content-addressed metadata table. The actual storage path is
// deterministic from the hash: `{hash[0:2]}/{hash[2:4]}/{hash}`.
// No need to record storage_type, bucket, or path — they come from
// runtime config.

export const tapeFiles = pgTable(
	"tape_files",
	{
		id: serial("id").primaryKey(),
		/** SHA-256 hex — global content-addressed lookup key. */
		fileHash: varchar("file_hash", { length: 64 }).notNull().unique(),
		sizeBytes: integer("size_bytes").notNull(),
		mimeType: text("mime_type").notNull(),
		createdAt: bigint("created_at", { mode: "number" }).notNull(),
	},
	(t) => ({
		hashIdx: index("idx_tape_files_hash").on(t.fileHash),
	}),
);

export type TapeFileRow = typeof tapeFiles.$inferSelect;
export type TapeFileInsert = typeof tapeFiles.$inferInsert;
