/**
 * TapeSlice — a filtered view of a Tape.
 *
 * Provides read-only access to a subset of tape entries,
 * determined by a filter function. Useful for:
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
import { TapeView } from "./view.ts";

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
	private readonly filter: (entry: TapeEntry) => boolean;
	private readonly lastCount?: number;

	constructor(tape: Tape, filter: (entry: TapeEntry) => boolean, lastCount?: number) {
		this.tape = tape;
		this.filter = filter;
		this.lastCount = lastCount;
	}

	get id(): string {
		return this.tape.ref.tapeId;
	}

	// ── Read operations ─────────────────────────────────────────────

	/**
	 * Get filtered entries from the parent tape.
	 * Loads all entries and applies the filter.
	 */
	async entries(): Promise<TapeEntry[]> {
		const all = await this.tape.entries();
		const filtered = all.filter(this.filter);
		return this.lastCount === undefined ? filtered : filtered.slice(-this.lastCount);
	}

	/** Convert this raw slice to a read-only view, applying tape edits before filtering. */
	view(): TapeView {
		return new TapeView(this.tape, this.filter);
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
		const entries = await this.tape.entries();
		return entries.some(this.filter);
	}

	// ── Composition ─────────────────────────────────────────────────

	and(filter: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this.tape, (e) => this.filter(e) && filter(e), this.lastCount);
	}

	or(filter: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this.tape, (e) => this.filter(e) || filter(e), this.lastCount);
	}

	not(): TapeSlice {
		return new TapeSlice(this.tape, (e) => !this.filter(e), this.lastCount);
	}

	// ── Common slice factories ──────────────────────────────────────

	last(n: number): TapeSlice {
		return new TapeSlice(this.tape, this.filter, n);
	}

	get parent(): Tape {
		return this.tape;
	}
}
