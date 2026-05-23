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
import type { TapeEntry, TapeInfo, TapeRef } from "./types.ts";
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
	 */
	async redact(entryIds: number | number[], reason?: string): Promise<void> {
		const ids = Array.isArray(entryIds) ? entryIds : [entryIds];
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
	 * Amend entries — replace content for LLM conversion.
	 * Original entries preserved in tape, model sees new content.
	 */
	async amend(entryIds: number | number[], content: unknown, reason?: string): Promise<void> {
		const ids = Array.isArray(entryIds) ? entryIds : [entryIds];
		for (const id of ids) {
			await this.append({
				kind: "amendment",
				payload: { targetId: id, content, reason, amendedAt: Date.now() },
				meta: {},
				date: new Date().toISOString(),
			});
		}
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

/**
 * Apply redactions and amendments to entries.
 * - Redacted entries → excluded (filtered out)
 * - Amended entries → use new content
 * - All other entries → pass through
 */
function applyEdits(entries: TapeEntry[]): TapeEntry[] {
	const redacted = new Set<number>();
	const amended = new Map<number, unknown>();

	for (const entry of entries) {
		if (entry.kind === "redaction") {
			const targetId = entry.payload.targetId as number;
			if (targetId) redacted.add(targetId);
		}
		if (entry.kind === "amendment") {
			const targetId = entry.payload.targetId as number;
			if (targetId) amended.set(targetId, entry.payload.content);
		}
	}

	// No edits → return as-is
	if (redacted.size === 0 && amended.size === 0) return entries;

	return entries.filter((entry) => {
		// Remove redaction/amendment marker entries
		if (entry.kind === "redaction" || entry.kind === "amendment") return false;
		// Remove redacted entries
		if (redacted.has(entry.id)) return false;
		return true;
	}).map((entry) => {
		// Apply amendment
		const newContent = amended.get(entry.id);
		if (newContent !== undefined) {
			return {
				...entry,
				payload: { ...entry.payload, content: newContent },
				meta: { ...entry.meta, amended: true },
			};
		}
		return entry;
	});
}
