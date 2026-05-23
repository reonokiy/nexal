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
import type { TapeEntry, TapeInfo } from "./types.ts";
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
}
