import type { TapeEntry } from "./types.ts";
import type { TapeContextManager, TapeLifecycleHooks, TapeSummaryManager, TapeSummaryManagerInput, TapeSummaryManagerResult } from "./tape.ts";
import { TapeEvent } from "./events.ts";

export interface TailContextManagerOptions {
	/** Hard cap used when loadContext() does not pass maxEntries. */
	maxEntries?: number;
}

export interface SummaryContextManagerOptions extends TailContextManagerOptions {
	/** Include the latest recorded summary before the recent tail window. Default true. */
	includeLatestSummary?: boolean;
}

export interface ExternalSummaryManagerOptions {
	summarize(input: TapeSummaryManagerInput): Promise<string | Record<string, unknown> | TapeSummaryManagerResult | null>
		| string
		| Record<string, unknown>
		| TapeSummaryManagerResult
		| null;
}

export interface AutoSummaryHooksOptions {
	/** Summarize after this many appended non-summary entries. Default 50. */
	everyEntries?: number;
	/** Do not summarize until the tape has at least this many entries. Default everyEntries. */
	minEntries?: number;
	/** Context window passed to updateSummary(). */
	maxEntries?: number;
	scope?: string;
	metadata?: Record<string, unknown>;
	reason?: string;
	/** Optional custom gate for app-specific policy. */
	shouldSummarize?(input: { entries: readonly TapeEntry[]; appended: readonly TapeEntry[] }): boolean;
}

export interface AppendThresholdHooksOptions {
	/** Trigger once this much appended context has accumulated. */
	threshold: number;
	/**
	 * Measures appended entries. Defaults to entry count; callers can provide
	 * character count, token estimate, byte size, or any other budget unit.
	 */
	measure?: (entry: TapeEntry) => number;
	/** Include/exclude entries from the threshold budget. Default includes all non-summary entries. */
	filter?: (entry: TapeEntry) => boolean;
	/** Called when the accumulated budget reaches threshold. */
	onThreshold(input: {
		entries: readonly TapeEntry[];
		appended: readonly TapeEntry[];
		amount: number;
		threshold: number;
		reset(): void;
	}): Promise<void> | void;
}

/** Default context policy: keep the most recent entries. */
export const tailContextManager: TapeContextManager = createTailContextManager();

export function createTailContextManager(options: TailContextManagerOptions = {}): TapeContextManager {
	return {
		select({ entries, maxEntries }) {
			const limit = options.maxEntries ?? maxEntries;
			if (entries.length <= limit) return entries;
			return entries.slice(-limit);
		},
	};
}

/**
 * Context policy for long conversations: include the latest recorded summary
 * plus a recent tail window, without duplicating the summary if it is already
 * inside the tail.
 */
export function createSummaryContextManager(options: SummaryContextManagerOptions = {}): TapeContextManager {
	const tail = createTailContextManager(options);
	return {
		async select(input) {
			const selected = Array.from(await tail.select(input));
			if (options.includeLatestSummary === false) return selected;
			const summary = latestSummary(input.entries);
			if (!summary || selected.some((entry) => entry.id === summary.id)) return selected;
			return [summary, ...selected];
		},
	};
}

/** Adapter for plugging any external summarizer into TapeSummaryManager. */
export function createExternalSummaryManager(options: ExternalSummaryManagerOptions): TapeSummaryManager {
	return {
		summarize(input) {
			return options.summarize(input);
		},
	};
}

/**
 * Event-driven strategy: observes append events and calls tape.updateSummary()
 * when the configured threshold is reached. Summary entries themselves do not
 * trigger another summary, which prevents feedback loops.
 */
export function createAutoSummaryHooks(options: AutoSummaryHooksOptions = {}): TapeLifecycleHooks {
	const everyEntries = options.everyEntries ?? 50;
	const minEntries = options.minEntries ?? everyEntries;
	let appendedSinceSummary = 0;
	let running = false;

	return {
		async onAppend(event) {
			const appended = event.entries.filter((entry) => !isSummaryEntry(entry));
			if (appended.length === 0) return;
			appendedSinceSummary += appended.length;
			const entries = await event.tape.view().entries();
			if (entries.length < minEntries) return;
			if (appendedSinceSummary < everyEntries) return;
			if (options.shouldSummarize && !options.shouldSummarize({ entries, appended })) return;
			if (running) return;
			running = true;
			try {
				const summary = await event.tape.updateSummary({
					maxEntries: options.maxEntries,
					scope: options.scope,
					metadata: options.metadata,
					reason: options.reason ?? "auto-summary",
					ifChanged: true,
				});
				if (summary) appendedSinceSummary = 0;
			} finally {
				running = false;
			}
		},
	};
}

/**
 * Event-driven threshold strategy: observes append events and calls onThreshold
 * after enough context has been appended. The default unit is entry count, but
 * callers can switch to characters/tokens via measure().
 */
export function createAppendThresholdHooks(options: AppendThresholdHooksOptions): TapeLifecycleHooks {
	let amount = 0;
	let running = false;
	const measure = options.measure ?? (() => 1);
	const filter = options.filter ?? ((entry: TapeEntry) => !isSummaryEntry(entry));

	return {
		async onAppend(event) {
			const appended = event.entries.filter(filter);
			if (appended.length === 0) return;
			amount += appended.reduce((sum, entry) => sum + measure(entry), 0);
			if (amount < options.threshold || running) return;
			running = true;
			try {
				await options.onThreshold({
					entries: await event.tape.view().entries(),
					appended,
					amount,
					threshold: options.threshold,
					reset() {
						amount = 0;
					},
				});
			} finally {
				running = false;
			}
		},
	};
}

function latestSummary(entries: readonly TapeEntry[]): TapeEntry | null {
	for (let i = entries.length - 1; i >= 0; i--) {
		const entry = entries[i]!;
		if (isSummaryEntry(entry)) return entry;
	}
	return null;
}

function isSummaryEntry(entry: TapeEntry): boolean {
	return entry.kind === "event" && entry.payload.name === TapeEvent.System.Summary;
}
