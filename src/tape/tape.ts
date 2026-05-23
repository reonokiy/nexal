/**
 * Tape — high-level wrapper around TapeStore.
 *
 * Provides an object-oriented interface for tape operations,
 * including conversion to LLM format and context window management.
 *
 * Tape is the canonical memory format. Conversion to LLM Message[]
 * happens only at the model boundary via `toMessages()`.
 *
 * Redactions and amendments are automatically applied when converting
 * to LLM format — redacted entries are excluded, amended entries use
 * their new content. All history is preserved in the tape.
 */
import type { Message } from "@mariozechner/pi-ai";
import type { TapeStore } from "./store.ts";
import type { TapeEntry, TapeInfo, TapeRef, TapeRange } from "./types.ts";
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
 * @example
 * ```typescript
 * const tape = await Tape.load(store, "session:123");
 *
 * // Convert to LLM format (auto-applies redactions/amendments)
 * const messages = await tape.toContext(200);
 *
 * // Edit tape
 * await tape.redact(42, { reason: "PII" });
 * await tape.amend(43, { content: "corrected" });
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

	static async load(store: TapeStore, name: string, maxContext?: number): Promise<Tape> {
		const tape = new Tape({ store, name, maxContext });
		await tape.load();
		return tape;
	}

	static async loadOrCreate(
		store: TapeStore,
		name: string,
		anchorState?: Record<string, unknown>,
		maxContext?: number,
	): Promise<Tape> {
		const tape = new Tape({ store, name, maxContext });
		const entries = await tape.load();
		if (entries.length === 0 && anchorState) {
			await tape.anchor("init", anchorState);
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

	/** Write an anchor (checkpoint) to the tape. */
	async anchor(name: string, state?: Record<string, unknown>): Promise<void> {
		await this.store.handoff(this.name, name, state);
	}

	/** Find an anchor by name. */
	async findAnchor(name: string): Promise<TapeEntry | null> {
		const entries = await this.load();
		return entries.findLast(
			(e) => e.kind === "anchor" && e.payload.name === name,
		) ?? null;
	}

	/** Load entries after a specific anchor. */
	async loadAfterAnchor(anchorName: string): Promise<TapeEntry[]> {
		const entries = await this.load();
		const idx = entries.findLastIndex(
			(e) => e.kind === "anchor" && e.payload.name === anchorName,
		);
		if (idx === -1) return [];
		return entries.slice(idx + 1);
	}

	/** Load entries between two anchors. */
	async loadBetween(fromAnchor: string, toAnchor: string): Promise<TapeEntry[]> {
		const entries = await this.load();
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
		return this.store.search(this.name, query, limit);
	}

	// ── Context management ──────────────────────────────────────────

	/**
	 * Load entries and truncate to context window.
	 */
	async loadContext(maxMessages?: number): Promise<TapeEntry[]> {
		const entries = await this.load();
		const limit = maxMessages ?? this.maxContext;
		if (entries.length <= limit) return entries;
		return entries.slice(-limit);
	}

	/**
	 * Convert entries to LLM Message format.
	 * Automatically applies redactions and amendments:
	 * - Redacted entries are excluded
	 * - Amended entries use new content
	 */
	toMessages(entries?: TapeEntry[]): Message[] {
		const raw = entries ?? [];
		const cleaned = applyEdits(raw);
		return entriesToLlmMessages(cleaned);
	}

	/**
	 * Load entries, truncate, and convert to LLM format.
	 * Primary method for model interaction.
	 */
	async toContext(maxMessages?: number): Promise<Message[]> {
		const entries = await this.loadContext(maxMessages);
		return this.toMessages(entries);
	}

	// ── Cross-tape references ───────────────────────────────────────

	async ref(
		targetTape: string,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: { ref: { tape: targetTape, relation, meta } satisfies TapeRef },
			meta: {},
			date: new Date().toISOString(),
		});
	}

	async refEntry(
		targetTape: string,
		entryId: number,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: { ref: { tape: targetTape, entryId, relation, meta } satisfies TapeRef },
			meta: {},
			date: new Date().toISOString(),
		});
	}

	async refAnchor(
		targetTape: string,
		anchorName: string,
		relation: TapeRef["relation"] = "link",
		meta?: Record<string, unknown>,
	): Promise<void> {
		await this.append({
			kind: "ref",
			payload: { ref: { tape: targetTape, anchorName, relation, meta } satisfies TapeRef },
			meta: {},
			date: new Date().toISOString(),
		});
	}

	async refs(): Promise<TapeRef[]> {
		const entries = await this.load();
		return entries
			.filter((e) => e.kind === "ref")
			.map((e) => e.payload.ref as TapeRef)
			.filter(Boolean);
	}

	async refsByRelation(relation: TapeRef["relation"]): Promise<TapeRef[]> {
		const allRefs = await this.refs();
		return allRefs.filter((r) => r.relation === relation);
	}

	async resolveRef(ref: TapeRef): Promise<Tape> {
		return new Tape({ store: this.store, name: ref.tape });
	}

	async resolveRefEntry(ref: TapeRef): Promise<TapeEntry | null> {
		if (!ref.entryId && !ref.anchorName) return null;
		const targetTape = await this.resolveRef(ref);
		const entries = await targetTape.load();

		if (ref.entryId) {
			return entries.find((e) => e.id === ref.entryId) ?? null;
		}
		if (ref.anchorName) {
			return entries.find(
				(e) => e.kind === "anchor" && e.payload.name === ref.anchorName,
			) ?? null;
		}
		return null;
	}

	// ── Redaction & Amendment ───────────────────────────────────────

	/**
	 * Redact entries — exclude from LLM conversion.
	 * Original entries preserved in tape, only hidden from model.
	 *
	 * @param target - Entry id, array of ids, or range { from, to }
	 * @param reason - Why entries were redacted
	 *
	 * @example
	 * ```typescript
	 * await tape.redact(42, "PII");
	 * await tape.redact([42, 43, 44], "PII");
	 * await tape.redact({ from: 10, to: 20 }, "sensitive data");
	 * ```
	 */
	async redact(target: number | number[] | TapeRange, reason?: string): Promise<void> {
		const ids = resolveRange(target);
		for (const id of ids) {
			await this.append({
				kind: "redaction",
				payload: { targetId: id, reason, redactedAt: Date.now() },
				meta: {},
				date: new Date().toISOString(),
			});
		}
	}

	/**
	 * Amend entries — replace with new tape entries for LLM conversion.
	 * Original entries preserved in tape, model sees replacement content.
	 *
	 * @param target - Entry id, array of ids, or range { from, to }
	 * @param replacement - New tape entries to use instead
	 * @param reason - Why entries were amended
	 *
	 * @example
	 * ```typescript
	 * // Replace single entry
	 * await tape.amend(42, [
	 *   { kind: "message", payload: { role: "assistant", content: "corrected" }, meta: {}, date: "..." },
	 * ]);
	 *
	 * // Replace range with new conversation
	 * await tape.amend({ from: 10, to: 20 }, [
	 *   { kind: "message", payload: { role: "user", content: "fixed question" }, meta: {}, date: "..." },
	 *   { kind: "message", payload: { role: "assistant", content: "fixed answer" }, meta: {}, date: "..." },
	 * ], "fix conversation");
	 * ```
	 */
	async amend(
		target: number | number[] | TapeRange,
		replacement: Omit<TapeEntry, "id">[],
		reason?: string,
	): Promise<void> {
		const ids = resolveRange(target);
		// Store one amendment entry that covers the whole range
		await this.append({
			kind: "amendment",
			payload: {
				targetIds: ids,
				replacement,
				reason,
				amendedAt: Date.now(),
			},
			meta: {},
			date: new Date().toISOString(),
		});
	}

	// ── Legacy compatibility ────────────────────────────────────────

	toAgentMessages(entries?: TapeEntry[]) {
		const e = entries ?? [];
		return entriesToMessages(e);
	}

	static fromAgentMessages(messages: any[]): Omit<TapeEntry, "id">[] {
		return messagesToEntries(messages).map((e) => ({
			...e,
			date: new Date().toISOString(),
		}));
	}

	// ── Slicing ─────────────────────────────────────────────────────

	slice(predicate: (entry: TapeEntry) => boolean): TapeSlice {
		return new TapeSlice(this, predicate);
	}

	sliceTime(from?: string | Date, to?: string | Date): TapeSlice {
		const fromTime = from ? new Date(from).getTime() : 0;
		const toTime = to ? new Date(to).getTime() : Infinity;
		return this.slice((e) => {
			const t = new Date(e.date).getTime();
			return t >= fromTime && t < toTime;
		});
	}

	sliceKind(...kinds: TapeEntry["kind"][]): TapeSlice {
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
				const entries = await tape.load();
				return { ref, tape, entries };
			}),
		);
	}
}

// ── Internal helpers ───────────────────────────────────────────────

/** Resolve a target (id, array, or range) to an array of ids. */
function resolveRange(target: number | number[] | TapeRange): number[] {
	if (typeof target === "number") return [target];
	if (Array.isArray(target)) return target;
	// TapeRange: { from, to } inclusive
	const ids: number[] = [];
	for (let i = target.from; i <= target.to; i++) {
		ids.push(i);
	}
	return ids;
}

/**
 * Apply redactions and amendments to entries.
 * - Redacted entries → excluded
 * - Amended entries → replaced with new entries
 * - All history preserved in tape
 */
function applyEdits(entries: TapeEntry[]): TapeEntry[] {
	const redacted = new Set<number>();
	// Amendment maps a set of target ids to replacement entries
	const amendments: Array<{ targetIds: Set<number>; replacement: TapeEntry[] }> = [];

	for (const entry of entries) {
		if (entry.kind === "redaction") {
			const targetId = entry.payload.targetId as number;
			if (targetId) redacted.add(targetId);
		}
		if (entry.kind === "amendment") {
			const targetIds = entry.payload.targetIds as number[];
			const replacement = entry.payload.replacement as TapeEntry[];
			if (targetIds?.length && replacement?.length) {
				amendments.push({
					targetIds: new Set(targetIds),
					replacement,
				});
			}
		}
	}

	// No edits → return as-is
	if (redacted.size === 0 && amendments.length === 0) return entries;

	// Build a set of all amended target ids
	const amendedIds = new Set<number>();
	for (const a of amendments) {
		for (const id of a.targetIds) {
			amendedIds.add(id);
		}
	}

	// Process entries
	const result: TapeEntry[] = [];
	let i = 0;

	while (i < entries.length) {
		const entry = entries[i]!;

		// Skip redaction/amendment markers
		if (entry.kind === "redaction" || entry.kind === "amendment") {
			i++;
			continue;
		}

		// Skip redacted entries
		if (redacted.has(entry.id)) {
			i++;
			continue;
		}

		// Check if this entry is part of an amendment
		if (amendedIds.has(entry.id)) {
			// Find the amendment that covers this entry
			const amendment = amendments.find((a) => a.targetIds.has(entry.id));
			if (amendment) {
				// Add all replacement entries
				result.push(...amendment.replacement);
				// Skip all entries covered by this amendment
				for (const id of amendment.targetIds) {
					amendedIds.delete(id);
				}
				// Skip entries until we pass the last amended id
				const maxId = Math.max(...amendment.targetIds);
				while (i < entries.length && entries[i]!.id <= maxId) {
					i++;
				}
				continue;
			}
		}

		result.push(entry);
		i++;
	}

	return result;
}
