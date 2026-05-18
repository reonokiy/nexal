/**
 * Tape storage types — mirrors the tape.systems fact model.
 *
 * Core primitives:
 *   Tape     — chronological sequence of facts per session/workspace
 *   Entry    — immutable fact record (message, tool_call, tool_result, event, anchor)
 *   Anchor   — logical checkpoint carrying state payload
 *   FileRef  — external storage reference for binary attachments
 */

/** A single immutable record on a tape. */
export interface TapeEntry {
	/** Monotonic entry id within this tape (1, 2, 3…). */
	id: number;
	/** Entry kind — see TapeEntryKind. */
	kind: TapeEntryKind;
	/** Structured payload (message dict, anchor state, event data, …). */
	payload: Record<string, unknown>;
	/** Optional metadata (source channel, turn id, …). */
	meta: Record<string, unknown>;
	/** ISO-8601 timestamp. */
	date: string;
}

export type TapeEntryKind =
	| "anchor"
	| "message"
	| "tool_call"
	| "tool_result"
	| "event";

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

/** Payload shape for an entry that carries a binary attachment. */
export interface AttachmentPayload {
	fileRef: string; // sha256 hex
}

/** Anchor state contract. */
export interface AnchorState {
	phase: string;
	summary?: string;
	nextSteps?: string[];
	sourceEntryIds?: number[];
	owner?: string;
	[key: string]: unknown;
}
