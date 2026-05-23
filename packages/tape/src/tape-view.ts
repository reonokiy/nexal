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
import type { TapeEntry } from "./types.ts";
import type { Tape } from "./tape.ts";
import { applyEdits } from "./apply-edits.ts";

export class TapeView {
	private readonly tape: Tape;

	constructor(tape: Tape) {
		this.tape = tape;
	}

	get name(): string {
		return this.tape.name;
	}

	/** Load all entries with edits applied. */
	async load(): Promise<TapeEntry[]> {
		const entries = await this.tape.load();
		return applyEdits(entries);
	}

	/**
	 * Load entries with edits applied, truncated to context window.
	 * This is the primary method for getting LLM-ready entries.
	 */
	async loadContext(maxMessages?: number): Promise<TapeEntry[]> {
		const entries = await this.load();
		if (!maxMessages || entries.length <= maxMessages) return entries;
		return entries.slice(-maxMessages);
	}

	/** Get the underlying tape (for writes or raw access). */
	get raw(): Tape {
		return this.tape;
	}
}
