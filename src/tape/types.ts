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
	name: string;
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

/**
 * Reference to another tape or a specific entry within a tape.
 * Used for cross-tape linking (e.g., worker referencing session context).
 */
export interface TapeRef {
	/** Target tape name. */
	tape: string;
	/** Specific entry id within the target tape (optional). */
	entryId?: number;
	/** Or reference by anchor name (optional). */
	anchorName?: string;
	/** Relationship to the referenced tape. */
	relation?: "context" | "parent" | "fork" | "link";
	/** Additional metadata for the reference. */
	meta?: Record<string, unknown>;
}

/** A range of entries on a tape (inclusive). */
export interface TapeRange {
	/** Start entry id (inclusive). */
	from: number;
	/** End entry id (inclusive). */
	to: number;
}
