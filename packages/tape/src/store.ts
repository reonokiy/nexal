import type { FileRef, TapeEntry, TapeEntryDraft, TapeHandle, TapeInfo } from "./types.ts";

export interface FileStore {
	/**
	 * Upload bytes to external storage and return a FileRef.
	 * If the same content already exists (hash collision) the upload
	 * is skipped and the existing ref is returned immediately.
	 */
	upload(
		data: Uint8Array | Buffer,
		mimeType: string,
		filename: string,
	): Promise<FileRef>;
	/** Download bytes by content hash, or null if not found. */
	download(fileHash: string): Promise<Uint8Array | null>;
	/** Get a presigned (or public) URL for a file by its content hash. */
	getUrl(fileHash: string): Promise<string | null>;
	/** Close any open connections. */
	close?(): Promise<void>;
}

export interface TapeStore {
	/** Create an empty tape and return its stable ref. */
	create(): Promise<TapeHandle>;
	/** Return persisted tape summaries. */
	listTapes(): Promise<TapeInfo[]>;
	/** Read every entry for a tape, ordered by entry_id. */
	read(tape: TapeHandle): Promise<TapeEntry[]>;
	/** Append entries to the tail of a tape (allocates entry_id). */
	append(tape: TapeHandle, entry: TapeEntryDraft): Promise<TapeEntry>;
	append(tape: TapeHandle, entries: TapeEntryDraft[]): Promise<TapeEntry[]>;
	/** Delete all entries for a persisted tape reference (hard reset). */
	reset(tape: TapeHandle): Promise<void>;
	/** Runtime summary (entry count, anchors, last anchor, …). */
	info(tape: TapeHandle): Promise<TapeInfo>;
	/** Write a new anchor entry (handoff). */
	handoff(tape: TapeHandle, name: string, state?: Record<string, unknown>): Promise<void>;
	/** Search entries by fuzzy text match in payload (simple LIKE). */
	search(tape: TapeHandle, query: string, limit?: number): Promise<TapeEntry[]>;
}
