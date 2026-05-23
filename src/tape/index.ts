/**
 * Tape module — append-only fact storage with LLM conversion.
 *
 * Core abstractions:
 * - TapeStore: low-level Postgres-backed storage interface
 * - Tape: high-level OOP wrapper with context management
 * - TapeSlice: filtered read-only view of a tape
 *
 * Tape is the canonical memory format. Conversion to LLM Message[]
 * happens only at the model boundary via Tape.toLlmMessages().
 */
export { Tape } from "./tape.ts";
export type { TapeOptions } from "./tape.ts";
export { TapeSlice } from "./slice.ts";
export type { TapeStore, TapeStoreOptions } from "./store.ts";
export { createTapeStore } from "./store.ts";
export type { TapeEntry, TapeEntryKind, TapeInfo, FileRef } from "./types.ts";
export { createFileStore } from "./file-store.ts";
export type { FileStore } from "./file-store.ts";

// Conversion functions (for advanced use cases)
export {
	entriesToLlmMessages,
	entriesToMessages,
	messagesToEntries,
	messagesToJson,
	jsonToMessages,
	truncateEntries,
	truncateMessages,
} from "./convert.ts";
