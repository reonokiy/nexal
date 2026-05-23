import type { TapeEntry } from "../tape/types.ts";

/** Tape-based memory store — TapeEntry is the canonical format. */
export interface MemoryStore {
	/** Load all tape entries for a session. */
	load(sessionKey: string): Promise<TapeEntry[]>;

	/** Append new entries to the tape. */
	append(sessionKey: string, entries: Omit<TapeEntry, "id">[]): Promise<void>;

	/** Reset tape and replace with new entries. */
	replace(sessionKey: string, entries: Omit<TapeEntry, "id">[]): Promise<void>;
}
