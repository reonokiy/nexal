/**
 * Tape — high-level wrapper around TapeStore.
 *
 * Provides an object-oriented interface for tape operations,
 * including conversion to LLM format and context window management.
 *
 * Tape is the canonical memory format. Conversion to LLM Message[]
 * happens only at the model boundary via `toMessages()`.
 */
import type { Message } from "@mariozechner/pi-ai";
import type { TapeStore } from "./store.ts";
import type { TapeEntry, TapeInfo, TapeRef } from "./types.ts";
import type { FileStore } from "./file-store.ts";
import { TapeSlice } from "./slice.ts";
import {
	entriesToLlmMessages,
	entriesToMessages,
	messagesToEntries,
} from "./convert.ts";

const DEFAULT_MAX_CONTEXT = 200;

export interface TapeOptions {
	store: TapeStore;
	name: string;
	/** Default max context window size. Default 200. */
	maxContext?: number;
}

/**
 * Tape — a named, append-only sequence of facts.
 *
 * Wraps TapeStore with a cleaner API and adds:
 * - Direct conversion to LLM Message format
 * - Context window truncation
 * - Slice-based filtering for partial visibility
 * - Cross-tape references
 *
 * @example
 * ```typescript
 * // Load from database
 * const tape = await Tape.load(store, "session:telegram:-1001");
 *
 * // Or create a reference
 * const tape = new Tape({ store, name: "session:123" });
 *
 * // Load and convert to LLM format
 * const messages = await tape.toContext(200);
 *
 * // Reference another tape
 * await tape.ref("worker:abc-123", "context");
 * ```
 */
export class Tape {
	readonly name: string;
	private readonly store: TapeStore;
	private readonly maxContext: number;

	constructor(opts: TapeOptions) {
		this.name = opts.name;
		this.store = opts.store;
		this.maxContext = opts.maxContext ?? DEFAULT_MAX_CONTEXT;
	}

	// ── Static factories ────────────────────────────────────────────

	/**
	 * Load a tape from the database by name.
	 * Returns a Tape instance ready to use.
	 *
	 * @example
	 * ```typescript
	 * const session = await Tape.load(store, "session:telegram:-1001");
	 * const entries = await session.load();
	 * ```
	 */
	static async load(store: TapeStore, name: string, maxContext?: number): Promise<Tape> {
		const tape = new Tape({ store, name, maxContext });
		// Verify the tape exists by loading it
		await tape.load();
		return tape;
	}

	/**
	 * Load or create a tape. If it doesn't exist, creates it with an initial anchor.
	 *
	 * @example
	 * ```typescript
	 * const session = await Tape.loadOrCreate(store, "session:telegram:-1001", {
	 *   owner: "human",
	 *   channel: "telegram",
	 * });
	 * ```
	 */
	static async loadOrCreate(
		store: TapeStore,
		name: string,
		anchorState?: Record<string, unknown>,
		maxContext?: number,
	): Promise<Tape> {
		const tape = new Tape({ store, name, maxContext });
		const entries = await tape.load();
		if (entries.length === 0 && anchorState) {
			await tape.handoff("init", anchorState);
		}
		return tape;
	}

	// ── Core operations ─────────────────────────────────────────────

	/** Load all entries for this tape. */
	async load(): Promise<TapeEntry[]> {
		return this.store.read(this.name);
	}

	/** Append entries to the tape. */
	async append(...entries: Omit<TapeEntry, "id">[]): Promise<void> {
		for (const entry of entries) {
			await this.store.append(this.name, entry);
		}
	}

	/** Delete all entries (hard reset). */
	async reset(): Promise<void> {
		await this.store.reset(this.name);
	}

	/** Get tape metadata. */
	async info(): Promise<TapeInfo> {
		return this.store.info(this.name);
	}

	/** Write a handoff anchor. */
	async handoff(name: string, state?: Record<string, unknown>): Promise<void> {
		await this.store.handoff(this.name, name, state);
	}

	/** Search entries by text pattern. */
	async search(query: string, limit?: number): Promise<TapeEntry[]> {
		return this.store.search(this.name, query, limit);
	}

	// ── Context management ──────────────────────────────────────────

	/**
	 * Load entries and truncate to context window.
	 * Returns the most recent entries that fit within maxContext.
	 */
	async loadContext(maxMessages?: number): Promise<TapeEntry[]> {
		const entries = await this.load();
		const limit = maxMessages ?? this.maxContext;
		if (entries.length <= limit) return entries;
		return entries.slice(-limit);
	}

	/**
	 * Convert entries to LLM Message format.
	 * If no entries provided, loads from tape.
	 */
	toMessages(entries?: TapeEntry[]): Message[] {
		const e = entries ?? [];
		return entriesToLlmMessages(e);
	}

	/**
	 * Load entries, truncate, and convert to LLM format in one call.
	 * This is the primary method for model interaction.
	 */
	async toContext(maxMessages?: number): Promise<Message[]> {
		const entries = await this.loadContext(maxMessages);
		return this.toMessages(entries);
	}

	// ── Cross-tape references ───────────────────────────────────────

	/**
	 * Create a reference from this tape to another tape.
	 * Stores a "ref" entry on this tape pointing to the target.
	 *
	 * @param targetTape - Name of the tape to reference
	 * @param relation - Relationship type (context, parent, fork, link)
	 * @param meta - Additional metadata for the reference
	 *
	 * @example
	 * ```typescript
	 * // Worker references the session tape for context
	 * await workerTape.ref("session:telegram:-1001", "context");
	 *
	 * // Sub-coordinator references parent coordinator
	 * await subTape.ref("coordinator:abc", "parent");
	 * ```
	 */
	async ref(
		targetTape: string,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: {
				ref: {
					tape: targetTape,
					relation,
					meta,
				} satisfies TapeRef,
			},
			meta: {},
			date: new Date().toISOString(),
		});
	}

	/**
	 * Create a reference to a specific entry in another tape.
	 *
	 * @param targetTape - Name of the target tape
	 * @param entryId - Specific entry id to reference
	 * @param relation - Relationship type
	 * @param meta - Additional metadata
	 */
	async refEntry(
		targetTape: string,
		entryId: number,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: {
				ref: {
					tape: targetTape,
					entryId,
					relation,
					meta,
				} satisfies TapeRef,
			},
			meta: {},
			date: new Date().toISOString(),
		});
	}

	/**
	 * Create a reference to an anchor in another tape.
	 *
	 * @param targetTape - Name of the target tape
	 * @param anchorName - Name of the anchor to reference
	 * @param relation - Relationship type
	 * @param meta - Additional metadata
	 */
	async refAnchor(
		targetTape: string,
		anchorName: string,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: {
				ref: {
					tape: targetTape,
					anchorName,
					relation,
					meta,
				} satisfies TapeRef,
			},
			meta: {},
			date: new Date().toISOString(),
		});
	}

	/**
	 * Get all references from this tape to other tapes.
	 * Returns an array of TapeRef objects.
	 */
	async refs(): Promise<TapeRef[]> {
		const entries = await this.load();
		return entries
			.filter((e) => e.kind === "ref")
			.map((e) => e.payload.ref as TapeRef)
			.filter(Boolean);
	}

	/**
	 * Get references filtered by relation type.
	 */
	async refsByRelation(relation: TapeRef["relation"]): Promise<TapeRef[]> {
		const allRefs = await this.refs();
		return allRefs.filter((r) => r.relation === relation);
	}

	/**
	 * Resolve a reference and load the target tape.
	 * Returns the target Tape instance.
	 */
	async resolveRef(ref: TapeRef): Promise<Tape> {
		return new Tape({ store: this.store, name: ref.tape });
	}

	/**
	 * Resolve a reference and load the specific entry.
	 * Returns the entry or null if not found.
	 */
	async resolveRefEntry(ref: TapeRef): Promise<TapeEntry | null> {
		if (!ref.entryId && !ref.anchorName) return null;

		const targetTape = await this.resolveRef(ref);
		const entries = await targetTape.load();

		if (ref.entryId) {
			return entries.find((e) => e.id === ref.entryId) ?? null;
		}

		if (ref.anchorName) {
			return (
				entries.find(
					(e) => e.kind === "anchor" && e.payload.name === ref.anchorName,
				) ?? null
			);
		}

		return null;
	}

	// ── Legacy compatibility ────────────────────────────────────────

	/** Convert entries to AgentMessage format (for pi-agent-core). */
	toAgentMessages(entries?: TapeEntry[]) {
		const e = entries ?? [];
		return entriesToMessages(e);
	}

	/** Convert AgentMessages to tape entries (for persistence). */
	static fromAgentMessages(messages: any[]): Omit<TapeEntry, "id">[] {
		return messagesToEntries(messages).map((e) => ({
			...e,
			date: new Date().toISOString(),
		}));
	}

	// ── Slicing ─────────────────────────────────────────────────────

	/**
	 * Create a filtered view of this tape.
	 * Only entries matching the predicate will be visible.
	 *
	 * @example
	 * ```typescript
	 * // Only user messages
	 * const userOnly = tape.slice(e => e.payload.role === "user");
	 *
	 * // Only entries after a timestamp
	 * const recent = tape.slice(e => e.date > cutoff);
	 *
	 * // Only specific kinds
	 * const messages = tape.slice(e => e.kind === "message");
	 * ```
	 */
	slice(predicate: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this, predicate);
	}

	/**
	 * Create a time-bounded slice.
	 * Only entries within [from, to) are visible.
	 */
	sliceTime(from?: string | Date, to?: string | Date): TapeSlice {
		const fromTime = from ? new Date(from).getTime() : 0;
		const toTime = to ? new Date(to).getTime() : Infinity;
		return this.slice((e) => {
			const t = new Date(e.date).getTime();
			return t >= fromTime && t < toTime;
		});
	}

	/**
	 * Create a kind-bounded slice.
	 * Only entries of the specified kinds are visible.
	 */
	sliceKind(...kinds: TapeEntry["kind"][]): TapeSlice {
		const kindSet = new Set(kinds);
		return this.slice((e) => kindSet.has(e.kind));
	}

	/**
	 * Create an entry-id-bounded slice.
	 * Only entries with id >= fromId are visible.
	 */
	sliceAfter(fromId: number): TapeSlice {
		return this.slice((e) => e.id >= fromId);
	}

	/**
	 * Get all referenced tapes and their resolved content.
	 * Returns an array of { ref, tape, entries } objects.
	 */
	async resolvedRefs(): Promise<Array<{ ref: TapeRef; tape: Tape; entries: TapeEntry[] }>> {
		const refs = await this.refs();
		const results = await Promise.all(
			refs.map(async (ref) => {
				const tape = await this.resolveRef(ref);
				const entries = await tape.load();
				return { ref, tape, entries };
			}),
		);
		return results;
	}
}
