/**
 * Tape — high-level wrapper around TapeStore.
 *
 * Provides an object-oriented interface for tape operations.
 * Tape is the canonical memory format — this class only deals
 * with TapeEntry[], conversion to LLM format happens at the
 * model boundary on the consumer side.
 *
 * Redactions and amendments are automatically applied when
 * calling loadContext() — redacted entries are excluded,
 * amended entries use their new content. All history is
 * preserved in the tape.
 */
import type { TapeStore } from "./store.ts";
import type { TapeEntry, TapeEntryDraft, TapeHandle, TapeInfo, TapeRef, TapeRange, TapeRelation } from "./types.ts";
import { TapeSlice } from "./slice.ts";
import { TapeView } from "./view.ts";
import { tapeRecord } from "./records.ts";

const DEFAULT_MAX_CONTEXT = 200;

export interface TapeOptions {
	store: TapeStore;
	ref: TapeHandle;
	/** Default max context window size. Default 200. */
	maxContext?: number;
}

/**
 * Tape — a named, append-only sequence of facts.
 *
 * @example
 * ```typescript
 * const tape = await Tape.load(store, "session:123");
 *
 * // Load entries with redactions/amendments applied
 * const entries = await tape.loadContext(200);
 *
 * // Edit tape
 * await tape.redact(42, { reason: "PII" });
 * await tape.amend(43, { content: "corrected" });
 * ```
 */
export class Tape {
	readonly ref: TapeHandle;
	private readonly store: TapeStore;
	private readonly maxContext: number;

	constructor(opts: TapeOptions) {
		this.ref = opts.ref;
		this.store = opts.store;
		this.maxContext = opts.maxContext ?? DEFAULT_MAX_CONTEXT;
	}

	// ── Static factories ────────────────────────────────────────────

	static async create(store: TapeStore, maxContext?: number): Promise<Tape> {
		return new Tape({ store, ref: await store.create(), maxContext });
	}

	static async load(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<Tape> {
		const tape = new Tape({ store, ref, maxContext });
		await tape.entries();
		return tape;
	}

	static async loadOrCreate(
		store: TapeStore,
		ref: TapeHandle,
		maxContext?: number,
	): Promise<Tape> {
		const tape = new Tape({ store, ref, maxContext });
		return tape;
	}

	// ── Core operations ─────────────────────────────────────────────

	/** Return all raw entries for this tape. */
	async entries(): Promise<TapeEntry[]> {
		return this.store.read(this.ref);
	}

	/** Append one entry or an atomic batch to the tape. */
	async append(entry: TapeEntryDraft): Promise<TapeEntry>;
	async append(entries: TapeEntryDraft[]): Promise<TapeEntry[]>;
	async append(entryOrEntries: TapeEntryDraft | TapeEntryDraft[]): Promise<TapeEntry | TapeEntry[]> {
		return Array.isArray(entryOrEntries)
			? this.store.append(this.ref, entryOrEntries)
			: this.store.append(this.ref, entryOrEntries);
	}

	/** Delete all entries (hard reset). */
	async reset(): Promise<void> {
		await this.store.reset(this.ref);
	}

	/** Get tape metadata. */
	async info(): Promise<TapeInfo> {
		return this.store.info(this.ref);
	}

	/** Write an anchor (checkpoint) to the tape. */
	async anchor(name: string, state?: Record<string, unknown>): Promise<void> {
		await this.store.handoff(this.ref, name, state);
	}

	/** Find an anchor by name. */
	async findAnchor(name: string): Promise<TapeEntry | null> {
		const entries = await this.entries();
		return entries.findLast(
			(e) => e.kind === "anchor" && e.payload.name === name,
		) ?? null;
	}

	/** Load entries after a specific anchor. */
	async loadAfterAnchor(anchorName: string): Promise<TapeEntry[]> {
		const entries = await this.entries();
		const idx = entries.findLastIndex(
			(e) => e.kind === "anchor" && e.payload.name === anchorName,
		);
		if (idx === -1) return [];
		return entries.slice(idx + 1);
	}

	/** Load entries between two anchors. */
	async loadBetween(fromAnchor: string, toAnchor: string): Promise<TapeEntry[]> {
		const entries = await this.entries();
		const fromIdx = entries.findIndex(
			(e) => e.kind === "anchor" && e.payload.name === fromAnchor,
		);
		const toIdx = entries.findLastIndex(
			(e) => e.kind === "anchor" && e.payload.name === toAnchor,
		);
		if (fromIdx === -1 || toIdx === -1 || fromIdx >= toIdx) return [];
		return entries.slice(fromIdx + 1, toIdx);
	}

	/** Search entries by text pattern. */
	async search(query: string, limit?: number): Promise<TapeEntry[]> {
		return this.store.search(this.ref, query, limit);
	}

	/** Return a read-only view with redactions/amendments applied. */
	view(): TapeView {
		return new TapeView(this);
	}

	// ── Context management ──────────────────────────────────────────

	/**
	 * Load entries and apply redactions/amendments, then truncate to
	 * context window. This is the primary method for consumers.
	 */
	async loadContext(maxMessages?: number): Promise<readonly TapeEntry[]> {
		const cleaned = await this.view().entries();
		const limit = maxMessages ?? this.maxContext;
		if (cleaned.length <= limit) return cleaned;
		return cleaned.slice(-limit);
	}

	// ── Cross-tape references ───────────────────────────────────────

	async link(
		targetTape: TapeHandle,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "tape", ...targetTape, relation, meta }));
	}

	async linkEntry(
		targetTape: TapeHandle,
		entryId: number,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "entry", ...targetTape, entryId, relation, meta }));
	}

	async linkAnchor(
		targetTape: TapeHandle,
		anchorName: string,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "anchor", ...targetTape, anchorName, relation, meta }));
	}

	async refs(): Promise<TapeRef[]> {
		const entries = await this.entries();
		return entries
			.filter((e) => e.kind === "ref")
			.map((e) => e.payload.ref as TapeRef)
			.filter(Boolean);
	}

	async refsByRelation(relation: TapeRelation): Promise<TapeRef[]> {
		const allRefs = await this.refs();
		return allRefs.filter((r) => r.relation === relation);
	}

	async resolveRef(ref: TapeRef): Promise<Tape> {
		return new Tape({ store: this.store, ref });
	}

	async resolveRefEntry(ref: TapeRef): Promise<TapeEntry | null> {
		if (ref.type === "tape" || ref.type === "range") return null;
		const targetTape = await this.resolveRef(ref);
		const entries = await targetTape.entries();

		if (ref.type === "entry") {
			return entries.find((e) => e.id === ref.entryId) ?? null;
		}
		if (ref.type === "anchor") {
			return entries.find(
				(e) => e.kind === "anchor" && e.payload.name === ref.anchorName,
			) ?? null;
		}
		return null;
	}

	// ── Redaction & Amendment ───────────────────────────────────────

	/**
	 * Redact entries — exclude from loadContext().
	 * Original entries preserved in tape, only hidden from context.
	 */
	async redact(target: number | number[] | TapeRange, reason?: string): Promise<void> {
		const ids = resolveRange(target);
		for (const id of ids) {
			await this.append(tapeRecord.redaction(id, reason));
		}
	}

	/**
	 * Amend entries — replace with new tape entries for context.
	 * Original entries preserved in tape, context sees replacement content.
	 */
	async amend(
		target: number | number[] | TapeRange,
		replacement: TapeEntryDraft[],
		reason?: string,
	): Promise<void> {
		const ids = resolveRange(target);
		await this.append(tapeRecord.amendment(ids, replacement, reason));
	}

	// ── Slicing ─────────────────────────────────────────────────────

	slice(filter: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this, filter);
	}

	sliceByTime(from?: string | Date, to?: string | Date): TapeSlice {
		const fromTime = from ? new Date(from).getTime() : 0;
		const toTime = to ? new Date(to).getTime() : Infinity;
		return this.slice((e) => {
			const t = new Date(e.date).getTime();
			return t >= fromTime && t < toTime;
		});
	}

	sliceByKind(...kinds: TapeEntry["kind"][]): TapeSlice {
		const kindSet = new Set(kinds);
		return this.slice((e) => kindSet.has(e.kind));
	}

	sliceAfter(fromId: number): TapeSlice {
		return this.slice((e) => e.id >= fromId);
	}

	async resolvedRefs(): Promise<Array<{ ref: TapeRef; tape: Tape; entries: TapeEntry[] }>> {
		const refs = await this.refs();
		return Promise.all(
			refs.map(async (ref) => {
				const tape = await this.resolveRef(ref);
		const entries = await tape.entries();
				return { ref, tape, entries };
			}),
		);
	}
}

// ── Internal helpers ───────────────────────────────────────────────

function resolveRange(target: number | number[] | TapeRange): number[] {
	if (typeof target === "number") return [target];
	if (Array.isArray(target)) return target;
	const ids: number[] = [];
	for (let i = target.from; i <= target.to; i++) {
		ids.push(i);
	}
	return ids;
}
