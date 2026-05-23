/**
 * TapeView — a read-only view of a Tape with redactions and amendments applied.
 *
 * Wraps a Tape and transparently applies edits on read:
 * - Redacted entries are excluded
 * - Amended entries use their replacement content
 * - All original history is preserved in the underlying Tape
 *
 * This is the primary interface for consuming tape data.
 */
import type { TapeEntry, TapeHandle } from "./types.ts";

interface TapeReader {
	readonly ref: TapeHandle;
	entries(): Promise<TapeEntry[]>;
}

export class TapeView {
	private readonly tape: TapeReader;
	private readonly filter?: (entry: TapeEntry) => boolean;

	constructor(tape: TapeReader, filter?: (entry: TapeEntry) => boolean) {
		this.tape = tape;
		this.filter = filter;
	}

	get id(): string {
		return this.tape.ref.tapeId;
	}

	/** Return all entries with edits applied. */
	async entries(): Promise<readonly TapeEntry[]> {
		const entries = await this.tape.entries();
		const edited = this.applyEdits(entries);
		return this.filter ? edited.filter(this.filter) : edited;
	}

	/**
	 * Load entries with edits applied, truncated to context window.
	 * This is the primary method for getting LLM-ready entries.
	 */
	async loadContext(maxMessages?: number): Promise<readonly TapeEntry[]> {
		const entries = await this.entries();
		if (!maxMessages || entries.length <= maxMessages) return entries;
		return entries.slice(-maxMessages);
	}

	private applyEdits(entries: readonly TapeEntry[]): TapeEntry[] {
		const redacted = new Set<number>();
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

		if (redacted.size === 0 && amendments.length === 0) return [...entries];

		const amendedIds = new Set<number>();
		for (const amendment of amendments) {
			for (const id of amendment.targetIds) {
				amendedIds.add(id);
			}
		}

		const result: TapeEntry[] = [];
		let i = 0;

		while (i < entries.length) {
			const entry = entries[i]!;

			if (entry.kind === "redaction" || entry.kind === "amendment") {
				i++;
				continue;
			}

			if (redacted.has(entry.id)) {
				i++;
				continue;
			}

			if (amendedIds.has(entry.id)) {
				const amendment = amendments.find((a) => a.targetIds.has(entry.id));
				if (amendment) {
					result.push(...amendment.replacement);
					for (const id of amendment.targetIds) {
						amendedIds.delete(id);
					}
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
}
