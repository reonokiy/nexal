/**
 * TapeSlice — a filtered view of a Tape.
 *
 * Provides read-only access to a subset of tape entries,
 * determined by a predicate function. Useful for:
 * - Filtering by message role (user/assistant/tool)
 * - Time-bounded views
 * - Kind-based filtering (messages only, tool results only)
 * - Incremental reads (entries after a certain id)
 *
 * A slice does not copy data — it reads from the parent tape
 * and filters in-memory.
 */
import type { TapeEntry } from "./types.ts";
import type { Tape } from "./tape.ts";

/**
 * A filtered, read-only view of a Tape.
 *
 * @example
 * ```typescript
 * const tape = new Tape({ store, name: "session:123" });
 *
 * // Create a slice with only user messages
 * const userSlice = tape.slice(e => e.payload.role === "user");
 *
 * // Get filtered entries
 * const userEntries = await userSlice.entries();
 * ```
 */
export class TapeSlice {
	private readonly tape: Tape;
	private readonly predicate: (entry: TapeEntry) => boolean;

	constructor(tape: Tape, predicate: (entry: TapeEntry) => boolean) {
		this.tape = tape;
		this.predicate = predicate;
	}

	// ── Read operations ─────────────────────────────────────────────

	/**
	 * Get filtered entries from the parent tape.
	 * Loads all entries and applies the predicate filter.
	 */
	async entries(): Promise<TapeEntry[]> {
		const all = await this.tape.load();
		return all.filter(this.predicate);
	}

	/**
	 * Get filtered entries with context window truncation.
	 * Returns the most recent filtered entries that fit within maxMessages.
	 */
	async context(maxMessages?: number): Promise<TapeEntry[]> {
		const filtered = await this.entries();
		if (!maxMessages || filtered.length <= maxMessages) return filtered;
		return filtered.slice(-maxMessages);
	}

	/**
	 * Count entries matching the filter.
	 */
	async count(): Promise<number> {
		const entries = await this.entries();
		return entries.length;
	}

	/**
	 * Check if any entries match the filter.
	 */
	async hasEntries(): Promise<boolean> {
		const entries = await this.tape.load();
		return entries.some(this.predicate);
	}

	// ── Composition ─────────────────────────────────────────────────

	and(predicate: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this.tape, (e) => this.predicate(e) && predicate(e));
	}

	or(predicate: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this.tape, (e) => this.predicate(e) || predicate(e));
	}

	not(): TapeSlice {
		return new TapeSlice(this.tape, (e) => !this.predicate(e));
	}

	// ── Common slice factories ──────────────────────────────────────

	last(n: number): TapeSlice {
		const parent = this;
		return new TapeSliceLast(parent, n);
	}

	get parent(): Tape {
		return this.tape;
	}
}

/**
 * Internal class for last-N slicing.
 * Overrides context() to return only the last N entries.
 */
class TapeSliceLast extends TapeSlice {
	private readonly parentSlice: TapeSlice;
	private readonly n: number;

	constructor(parent: TapeSlice, n: number) {
		super(parent.parent, () => true);
		this.parentSlice = parent;
		this.n = n;
	}

	override async entries(): Promise<TapeEntry[]> {
		const all = await this.parentSlice.entries();
		return all.slice(-this.n);
	}

	override async context(maxMessages?: number): Promise<TapeEntry[]> {
		const lastN = await this.entries();
		if (!maxMessages || lastN.length <= maxMessages) return lastN;
		return lastN.slice(-maxMessages);
	}
}
