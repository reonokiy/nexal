/**
 * Tape module — generic @nexal/tape APIs plus Nexal-specific semantics and storage.
 */
export * from "@nexal/tape";
export {
	createFileStore,
	createTapeStore,
	getOrCreateSessionTapeRef,
	getSessionTapeRef,
	NexalTape,
	NexalTape as Tape,
} from "./nexal-tape.ts";
export type { NexalSessionContext, NexalWorkerContext, TapeStoreOptions } from "./nexal-tape.ts";
