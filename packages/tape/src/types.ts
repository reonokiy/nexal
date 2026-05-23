/**
 * Tape storage types — mirrors the tape.systems fact model.
 *
 * Core primitives:
 *   Tape     — chronological sequence of facts per session/workspace
 *   Entry    — immutable fact record (message, tool_call, tool_result, event, anchor)
 *   Anchor   — logical checkpoint carrying state payload
 *   FileRef  — external storage reference for binary attachments
 *   TapeRef  — reference to another tape or entry within a tape
 */

/** A single immutable record on a tape. */
export interface TapeEntry {
	/** Monotonic entry id within this tape (1, 2, 3…). */
	id: number;
	/** Entry kind — see TapeEntryKind. */
	kind: TapeEntryKind;
	/** Structured payload (message dict, anchor state, event data, …). */
	payload: Record<string, unknown>;
	/** Optional metadata (source channel, turn id, tape refs, …). */
	meta: Record<string, unknown>;
	/** ISO-8601 timestamp. */
	date: string;
}

/** Entry payload before the store assigns a tape-local id. */
export type TapeEntryDraft = Omit<TapeEntry, "id">;

export type TapeEntryKind =
	| "anchor"
	| "message"
	| "tool_call"
	| "tool_result"
	| "event"
	| "ref"
	| "redaction"
	| "amendment";

/** Runtime summary for a tape (returned by tape.info). */
export interface TapeInfo {
	/** Stable globally unique tape id. */
	id: string;
	entries: number;
	anchors: number;
	lastAnchor: string | null;
	entriesSinceLastAnchor: number;
	lastTokenUsage: number | null;
}

/** Reference to an externally-stored binary file. */
export interface FileRef {
	/** SHA-256 hex of the file content (also the lookup key). */
	fileHash: string;
	/** MIME type, e.g. "image/jpeg". */
	mimeType: string;
	/** Original filename (best-effort). */
	filename: string;
	/** Byte size. */
	sizeBytes: number;
	/** Presigned URL or public URL (short-lived). */
	url?: string;
}

/** Stable handle used to read/write a tape. */
export interface TapeHandle {
	/** Stable globally unique tape id. */
	tapeId: string;
	meta?: Record<string, unknown>;
}

export type TapeRelation = "context" | "parent" | "fork" | "link";

export type TapeRef = TapeWholeRef | TapeEntryRef | TapeAnchorRef | TapeRangeRef;

export interface TapeWholeRef extends TapeHandle {
	type: "tape";
	/** Relationship to the referenced tape. */
	relation?: TapeRelation;
}

export interface TapeEntryRef extends TapeHandle {
	type: "entry";
	/** Specific entry id within the target tape (optional). */
	entryId: number;
	/** Relationship to the referenced tape. */
	relation?: TapeRelation;
}

export interface TapeAnchorRef extends TapeHandle {
	type: "anchor";
	/** Reference by anchor name. */
	anchorName: string;
	/** Relationship to the referenced tape. */
	relation?: TapeRelation;
}

export interface TapeRangeRef extends TapeHandle {
	type: "range";
	/** Start entry id (inclusive). */
	from: number;
	/** End entry id (inclusive). */
	to: number;
	/** Relationship to the referenced tape. */
	relation?: TapeRelation;
}

/** A range of entries on a tape (inclusive). */
export interface TapeRange {
	/** Start entry id (inclusive). */
	from: number;
	/** End entry id (inclusive). */
	to: number;
}
