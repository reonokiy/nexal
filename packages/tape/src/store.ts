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
