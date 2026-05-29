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
import type { AssistantMessageRecordOptions, TapeMessagePayload, TapeRecordOptions, ToolResultRecordInput } from "./records.ts";
import { messagesToEntries } from "./convert.ts";
import { tailContextManager } from "./strategies.ts";
import { TapeChange, TapeEvent, TapeStatus, type TapeConfigStatus } from "./events.ts";

const DEFAULT_MAX_CONTEXT = 200;

const TapeSystemRecord = {
	SystemPrompt: { name: "systemPrompt", event: TapeEvent.System.Prompt },
	Model: { name: "model", event: TapeEvent.System.Model },
	Tools: { name: "tools", event: TapeEvent.System.Tools },
	Context: { name: "context", event: TapeEvent.System.Context, payloadKey: "context" },
	Policy: { name: "policy", event: TapeEvent.System.Policy, payloadKey: "policy" },
	Runtime: { name: "runtime", event: TapeEvent.System.Runtime, payloadKey: "runtime" },
	Status: { name: "status", event: TapeEvent.System.Status },
} as const;

const TapeLifecycleEventType = {
	Append: "append",
	ContextLoaded: "context:loaded",
	SystemRecorded: "system:recorded",
	SummaryRecorded: "summary:recorded",
	SummaryUpdated: "summary:updated",
} as const;

type TapeSystemRecordSpec = typeof TapeSystemRecord[keyof typeof TapeSystemRecord];
type TapeScopedRecordSpec =
	| typeof TapeSystemRecord.Context
	| typeof TapeSystemRecord.Policy
	| typeof TapeSystemRecord.Runtime;

export interface TapeSystemOptions {
	scope?: string;
	metadata?: Record<string, unknown>;
	/** When true, skip appending if the latest matching value is already current. */
	ifChanged?: boolean;
}

export interface TapeSystemPromptOptions extends TapeSystemOptions {}

export interface TapeModelOptions extends TapeSystemOptions {}

export interface TapeToolsOptions extends TapeSystemOptions {}

export interface TapeContextOptions extends TapeSystemOptions {}

export interface TapePolicyOptions extends TapeSystemOptions {}

export interface TapeRuntimeOptions extends TapeSystemOptions {}

export interface TapeStatusOptions extends TapeSystemOptions {}

export interface TapeSummaryOptions extends TapeSystemOptions {
	/** Entry range covered by this summary, inclusive. */
	range?: TapeRange;
}

export interface TapeMessageRecordOptions extends TapeRecordOptions {}

export interface TapeUserMessageOptions extends TapeRecordOptions {
	timestamp?: number;
}

export interface TapeLinkOptions extends TapeRecordOptions {}

type MaybePromise<T> = T | Promise<T>;

export interface TapeLoadContextOptions {
	/** Maximum entries requested from the context manager. Defaults to TapeOptions.maxContext. */
	maxEntries?: number;
	/** Optional reason from the caller, e.g. "llm-call", "search", "resume". */
	reason?: string;
	metadata?: Record<string, unknown>;
}

export interface TapeContextManagerInput {
	tape: Tape;
	/** Redactions/amendments have already been applied. */
	entries: readonly TapeEntry[];
	maxEntries: number;
	reason?: string;
	metadata?: Record<string, unknown>;
}

export interface TapeContextManager {
	select(input: TapeContextManagerInput): MaybePromise<readonly TapeEntry[]>;
}

export interface TapeSummaryManagerInput {
	tape: Tape;
	/** Redactions/amendments have already been applied. */
	entries: readonly TapeEntry[];
	/** The current selected context window, if the caller did not provide one. */
	context: readonly TapeEntry[];
	/** Entry range this summary is expected to cover, inferred from context unless provided. */
	range?: TapeRange;
	scope?: string;
	metadata?: Record<string, unknown>;
	reason?: string;
}

export interface TapeSummaryManagerResult {
	summary: string | Record<string, unknown>;
	/** Entry range covered by this summary. Overrides the inferred/update option range. */
	range?: TapeRange;
	scope?: string;
	metadata?: Record<string, unknown>;
}

export interface TapeSummaryManager {
	summarize(input: TapeSummaryManagerInput): MaybePromise<string | Record<string, unknown> | TapeSummaryManagerResult | null>;
}

export interface TapeUpdateSummaryOptions extends TapeSummaryOptions {
	context?: readonly TapeEntry[];
	maxEntries?: number;
	reason?: string;
}

export type TapeSystemRecordName = TapeSystemRecordSpec["name"];

export interface TapeAppendEvent {
	type: typeof TapeLifecycleEventType.Append;
	tape: Tape;
	entries: readonly TapeEntry[];
}

export interface TapeContextLoadedEvent {
	type: typeof TapeLifecycleEventType.ContextLoaded;
	tape: Tape;
	entries: readonly TapeEntry[];
	context: readonly TapeEntry[];
	options: TapeLoadContextOptions;
}

export interface TapeSystemRecordedEvent {
	type: typeof TapeLifecycleEventType.SystemRecorded;
	tape: Tape;
	name: TapeSystemRecordName;
	entry: TapeEntry;
	data: Record<string, unknown>;
}

export interface TapeSummaryRecordedEvent {
	type: typeof TapeLifecycleEventType.SummaryRecorded;
	tape: Tape;
	entry: TapeEntry;
	summary: string | Record<string, unknown>;
	data: Record<string, unknown>;
}

export interface TapeSummaryUpdatedEvent {
	type: typeof TapeLifecycleEventType.SummaryUpdated;
	tape: Tape;
	entry: TapeEntry | null;
	result: TapeSummaryManagerResult | null;
	options: TapeUpdateSummaryOptions;
}

export type TapeLifecycleEvent =
	| TapeAppendEvent
	| TapeContextLoadedEvent
	| TapeSystemRecordedEvent
	| TapeSummaryRecordedEvent
	| TapeSummaryUpdatedEvent;

export interface TapeLifecycleHooks {
	onEvent?(event: TapeLifecycleEvent): MaybePromise<void>;
	onAppend?(event: TapeAppendEvent): MaybePromise<void>;
	onContextLoaded?(event: TapeContextLoadedEvent): MaybePromise<void>;
	onSystemRecorded?(event: TapeSystemRecordedEvent): MaybePromise<void>;
	onSummaryRecorded?(event: TapeSummaryRecordedEvent): MaybePromise<void>;
	onSummaryUpdated?(event: TapeSummaryUpdatedEvent): MaybePromise<void>;
	onHookError?(error: unknown, event: TapeLifecycleEvent): MaybePromise<void>;
}

export type TapeHookErrorPolicy = "ignore" | "throw";

export interface TapeOptions {
	store: TapeStore;
	ref: TapeHandle;
	/** Default max context window size. Default 200. */
	maxContext?: number;
	/** Optional module that decides which entries are sent to the model. */
	contextManager?: TapeContextManager;
	/** Optional module that produces summaries/checkpoints. */
	summaryManager?: TapeSummaryManager;
	/** Optional lifecycle hooks for modules that react to tape activity. */
	hooks?: TapeLifecycleHooks;
	/** Whether lifecycle hook errors should fail the core tape operation. Default "ignore". */
	hookErrorPolicy?: TapeHookErrorPolicy;
}

export type TapeFactoryOptions = Omit<TapeOptions, "store" | "ref">;

/**
 * Tape — a named, append-only sequence of facts.
 *
 * @example
 * ```typescript
 * const tape = await Tape.load(store, { tapeId: "session:123" });
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
	private readonly contextManager: TapeContextManager;
	private readonly summaryManager?: TapeSummaryManager;
	private readonly hooks?: TapeLifecycleHooks;
	private readonly hookErrorPolicy: TapeHookErrorPolicy;

	constructor(opts: TapeOptions) {
		this.ref = opts.ref;
		this.store = opts.store;
		this.maxContext = opts.maxContext ?? DEFAULT_MAX_CONTEXT;
		this.contextManager = opts.contextManager ?? tailContextManager;
		this.summaryManager = opts.summaryManager;
		this.hooks = opts.hooks;
		this.hookErrorPolicy = opts.hookErrorPolicy ?? "ignore";
	}

	// ── Static factories ────────────────────────────────────────────

	static async create(store: TapeStore, maxContext?: number): Promise<Tape>;
	static async create(store: TapeStore, options?: TapeFactoryOptions): Promise<Tape>;
	static async create(store: TapeStore, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<Tape> {
		return new Tape({ store, ref: await store.create(), ...factoryOptions(optionsOrMaxContext) });
	}

	static async load(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<Tape>;
	static async load(store: TapeStore, ref: TapeHandle, options?: TapeFactoryOptions): Promise<Tape>;
	static async load(store: TapeStore, ref: TapeHandle, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<Tape> {
		const tape = new Tape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
		await tape.entries();
		return tape;
	}

	static async loadOrCreate(
		store: TapeStore,
		ref: TapeHandle,
		maxContext?: number,
	): Promise<Tape>;
	static async loadOrCreate(
		store: TapeStore,
		ref: TapeHandle,
		options?: TapeFactoryOptions,
	): Promise<Tape>;
	static async loadOrCreate(
		store: TapeStore,
		ref: TapeHandle,
		optionsOrMaxContext?: number | TapeFactoryOptions,
	): Promise<Tape> {
		const tape = new Tape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
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
		const appended = Array.isArray(entryOrEntries)
			? await this.store.append(this.ref, entryOrEntries)
			: await this.store.append(this.ref, entryOrEntries);
		await this.emit({
			type: TapeLifecycleEventType.Append,
			tape: this,
			entries: Array.isArray(appended) ? appended : [appended],
		});
		return appended;
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

	/** Record a generic message entry. */
	async recordMessage(payload: TapeMessagePayload, options: TapeMessageRecordOptions = {}): Promise<TapeEntry> {
		return this.append(tapeRecord.message(payload, options));
	}

	/** Record a user prompt/message entry. */
	async recordUserMessage(content: unknown, options: TapeUserMessageOptions = {}): Promise<TapeEntry> {
		return this.append(tapeRecord.userMessage(content, options));
	}

	/** Record an assistant response entry. */
	async recordAssistantMessage(content: unknown, options: AssistantMessageRecordOptions = {}): Promise<TapeEntry> {
		return this.append(tapeRecord.assistantMessage(content, options));
	}

	/** Record a tool execution result entry. */
	async recordToolResult(input: ToolResultRecordInput, options: TapeRecordOptions = {}): Promise<TapeEntry> {
		return this.append(tapeRecord.toolResult(input, options));
	}

	/** Record a batch of structural LLM/agent messages. */
	async recordMessages(messages: readonly unknown[], options: TapeRecordOptions = {}): Promise<TapeEntry[]> {
		const date = options.date ?? new Date();
		const entries = messagesToEntries(messages).map((entry) => ({
			...entry,
			meta: { ...entry.meta, ...(options.meta ?? {}) },
			date: toIsoDate(date),
		}));
		if (entries.length === 0) return [];
		return this.append(entries);
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
	async loadContext(maxMessages?: number): Promise<readonly TapeEntry[]>;
	async loadContext(options?: TapeLoadContextOptions): Promise<readonly TapeEntry[]>;
	async loadContext(optionsOrMaxMessages?: number | TapeLoadContextOptions): Promise<readonly TapeEntry[]> {
		const cleaned = await this.view().entries();
		const options = typeof optionsOrMaxMessages === "number"
			? { maxEntries: optionsOrMaxMessages }
			: optionsOrMaxMessages ?? {};
		const context = await this.contextManager.select({
			tape: this,
			entries: cleaned,
			maxEntries: options.maxEntries ?? this.maxContext,
			reason: options.reason,
			metadata: options.metadata,
		});
		await this.emit({ type: TapeLifecycleEventType.ContextLoaded, tape: this, entries: cleaned, context, options });
		return context;
	}

	/** Record the system prompt used to drive this tape. */
	async setSystemPrompt(
		systemPrompt: string,
		options: TapeSystemPromptOptions = {},
	): Promise<TapeEntry | null> {
		const data = systemPromptData(systemPrompt, options);
		if (options.ifChanged && await this.systemPromptStatus(systemPrompt, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.SystemPrompt, data, options);
	}

	/** Compare a system prompt with the latest prompt recorded on this tape. */
	async systemPromptStatus(
		systemPrompt: string,
		options: Omit<TapeSystemPromptOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.SystemPrompt.event, systemPromptData(systemPrompt, options));
	}

	/** Record the model used to drive this tape. */
	async setModel(
		model: unknown,
		options: TapeModelOptions = {},
	): Promise<TapeEntry | null> {
		const data = modelData(model, options);
		if (options.ifChanged && await this.modelStatus(model, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Model, data, options);
	}

	/** Compare a model configuration with the latest one recorded on this tape. */
	async modelStatus(
		model: unknown,
		options: Omit<TapeModelOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Model.event, modelData(model, options));
	}

	/** Record the tool definitions available while driving this tape. */
	async setTools(
		tools: readonly unknown[],
		options: TapeToolsOptions = {},
	): Promise<TapeEntry | null> {
		const data = toolsData(tools, options);
		if (options.ifChanged && await this.toolsStatus(tools, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Tools, data, options);
	}

	/** Compare tool definitions with the latest ones recorded on this tape. */
	async toolsStatus(
		tools: readonly unknown[],
		options: Omit<TapeToolsOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Tools.event, toolsData(tools, options));
	}

	/** Record stable non-message context needed to understand or replay the tape. */
	async setContext(
		context: Record<string, unknown>,
		options: TapeContextOptions = {},
	): Promise<TapeEntry | null> {
		const data = scopedData(TapeSystemRecord.Context, context, options);
		if (options.ifChanged && await this.contextStatus(context, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Context, data, options);
	}

	/** Compare non-message context with the latest one recorded on this tape. */
	async contextStatus(
		context: Record<string, unknown>,
		options: Omit<TapeContextOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Context.event, scopedData(TapeSystemRecord.Context, context, options));
	}

	/** Record runtime policy that affects what the user sees and how the agent runs. */
	async setPolicy(
		policy: Record<string, unknown>,
		options: TapePolicyOptions = {},
	): Promise<TapeEntry | null> {
		const data = scopedData(TapeSystemRecord.Policy, policy, options);
		if (options.ifChanged && await this.policyStatus(policy, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Policy, data, options);
	}

	/** Compare policy context with the latest one recorded on this tape. */
	async policyStatus(
		policy: Record<string, unknown>,
		options: Omit<TapePolicyOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Policy.event, scopedData(TapeSystemRecord.Policy, policy, options));
	}

	/** Record execution environment details such as sandbox/container identity. */
	async setRuntime(
		runtime: Record<string, unknown>,
		options: TapeRuntimeOptions = {},
	): Promise<TapeEntry | null> {
		const data = scopedData(TapeSystemRecord.Runtime, runtime, options);
		if (options.ifChanged && await this.runtimeStatus(runtime, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Runtime, data, options);
	}

	/** Compare runtime context with the latest one recorded on this tape. */
	async runtimeStatus(
		runtime: Record<string, unknown>,
		options: Omit<TapeRuntimeOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Runtime.event, scopedData(TapeSystemRecord.Runtime, runtime, options));
	}

	/** Record lifecycle status transitions for this tape's owner. */
	async setStatus(
		status: string,
		options: TapeStatusOptions = {},
	): Promise<TapeEntry | null> {
		const data = statusData(status, options);
		if (options.ifChanged && await this.statusState(status, options) === TapeStatus.Current) {
			return null;
		}
		return this.appendSystemRecord(TapeSystemRecord.Status, data, options);
	}

	/** Compare lifecycle status with the latest one recorded on this tape. */
	async statusState(
		status: string,
		options: Omit<TapeStatusOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeSystemRecord.Status.event, statusData(status, options));
	}

	/** Record a compact summary/checkpoint produced by external code or a summary manager. */
	async recordSummary(
		summary: string | Record<string, unknown>,
		options: TapeSummaryOptions = {},
	): Promise<TapeEntry | null> {
		const data = summaryData(summary, options);
		if (options.ifChanged && await this.summaryStatus(summary, options) === TapeStatus.Current) {
			return null;
		}
		const entry = await this.append(systemEvent(TapeEvent.System.Summary, data, options));
		await this.emit({ type: TapeLifecycleEventType.SummaryRecorded, tape: this, entry, summary, data });
		return entry;
	}

	/** Compare a summary/checkpoint with the latest one recorded on this tape. */
	async summaryStatus(
		summary: string | Record<string, unknown>,
		options: Omit<TapeSummaryOptions, "ifChanged"> = {},
	): Promise<TapeConfigStatus> {
		const entries = await this.entries();
		return entryStatus(entries, TapeEvent.System.Summary, summaryData(summary, options));
	}

	/** Ask the configured summary manager to produce and record a summary. */
	async updateSummary(options: TapeUpdateSummaryOptions = {}): Promise<TapeEntry | null> {
		if (!this.summaryManager) {
			await this.emit({ type: TapeLifecycleEventType.SummaryUpdated, tape: this, entry: null, result: null, options });
			return null;
		}
		const entries = await this.view().entries();
		const context = options.context ?? await this.loadContext({
			maxEntries: options.maxEntries,
			reason: options.reason ?? "summary",
			metadata: options.metadata,
		});
		const range = options.range ?? rangeFromEntries(context);
		const result = await this.summaryManager.summarize({
			tape: this,
			entries,
			context,
			range,
			scope: options.scope,
			metadata: options.metadata,
			reason: options.reason,
		});
		if (!result) {
			await this.emit({ type: TapeLifecycleEventType.SummaryUpdated, tape: this, entry: null, result: null, options });
			return null;
		}
		const normalized = normalizeSummaryResult(result);
		const entry = await this.recordSummary(normalized.summary, {
			range: normalized.range ?? range,
			scope: normalized.scope ?? options.scope,
			metadata: normalized.metadata ?? options.metadata,
			ifChanged: options.ifChanged,
		});
		await this.emit({ type: TapeLifecycleEventType.SummaryUpdated, tape: this, entry, result: normalized, options });
		return entry;
	}

	// ── Cross-tape references ───────────────────────────────────────

	async link(
		targetTape: TapeHandle,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
		options: TapeLinkOptions = {},
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "tape", ...targetTape, relation, meta }, options));
	}

	async linkEntry(
		targetTape: TapeHandle,
		entryId: number,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
		options: TapeLinkOptions = {},
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "entry", ...targetTape, entryId, relation, meta }, options));
	}

	async linkAnchor(
		targetTape: TapeHandle,
		anchorName: string,
		relation: TapeRelation = "link",
		meta?: Record<string, unknown>,
		options: TapeLinkOptions = {},
	): Promise<void> {
		await this.append(tapeRecord.ref({ type: "anchor", ...targetTape, anchorName, relation, meta }, options));
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
		return new Tape({
			store: this.store,
			ref,
			maxContext: this.maxContext,
			contextManager: this.contextManager,
			summaryManager: this.summaryManager,
			hooks: this.hooks,
			hookErrorPolicy: this.hookErrorPolicy,
		});
	}

	private async appendSystemRecord(
		record: TapeSystemRecordSpec,
		data: Record<string, unknown>,
		options: TapeSystemOptions,
	): Promise<TapeEntry> {
		const entry = await this.append(systemEvent(record.event, data, options));
		await this.emit({ type: TapeLifecycleEventType.SystemRecorded, tape: this, name: record.name, entry, data });
		return entry;
	}

	private async emit(event: TapeLifecycleEvent): Promise<void> {
		if (!this.hooks) return;
		try {
			await this.hooks.onEvent?.(event);
			switch (event.type) {
				case TapeLifecycleEventType.Append:
					await this.hooks.onAppend?.(event);
					return;
				case TapeLifecycleEventType.ContextLoaded:
					await this.hooks.onContextLoaded?.(event);
					return;
				case TapeLifecycleEventType.SystemRecorded:
					await this.hooks.onSystemRecorded?.(event);
					return;
				case TapeLifecycleEventType.SummaryRecorded:
					await this.hooks.onSummaryRecorded?.(event);
					return;
				case TapeLifecycleEventType.SummaryUpdated:
					await this.hooks.onSummaryUpdated?.(event);
					return;
			}
		} catch (error) {
			if (this.hookErrorPolicy === "throw") throw error;
			try {
				await this.hooks.onHookError?.(error, event);
			} catch {}
		}
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

function normalizeSummaryResult(
	result: string | Record<string, unknown> | TapeSummaryManagerResult,
): TapeSummaryManagerResult {
	if (typeof result === "string") return { summary: result };
	if (isSummaryManagerResult(result)) return result;
	return { summary: result };
}

function isSummaryManagerResult(value: object): value is TapeSummaryManagerResult {
	return "summary" in value;
}

function factoryOptions(optionsOrMaxContext?: number | TapeFactoryOptions): TapeFactoryOptions {
	return typeof optionsOrMaxContext === "number"
		? { maxContext: optionsOrMaxContext }
		: optionsOrMaxContext ?? {};
}

function rangeFromEntries(entries: readonly TapeEntry[]): TapeRange | undefined {
	if (entries.length === 0) return undefined;
	return {
		from: entries[0]!.id,
		to: entries[entries.length - 1]!.id,
	};
}

function resolveRange(target: number | number[] | TapeRange): number[] {
	if (typeof target === "number") return [target];
	if (Array.isArray(target)) return target;
	const ids: number[] = [];
	for (let i = target.from; i <= target.to; i++) {
		ids.push(i);
	}
	return ids;
}

function systemPromptData(
	systemPrompt: string,
	options: Omit<TapeSystemPromptOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		systemPrompt,
		metadata: options.metadata ?? {},
	});
}

function modelData(
	model: unknown,
	options: Omit<TapeModelOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		model: serializeModel(model),
		metadata: options.metadata ?? {},
	});
}

function toolsData(
	tools: readonly unknown[],
	options: Omit<TapeToolsOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		tools: tools.map(serializeTool),
		metadata: options.metadata ?? {},
	});
}

function scopedData(
	record: TapeScopedRecordSpec,
	value: Record<string, unknown>,
	options: Omit<TapeSystemOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		[record.payloadKey]: toPlain(value),
		metadata: options.metadata ?? {},
	});
}

function statusData(
	status: string,
	options: Omit<TapeStatusOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		status,
		metadata: options.metadata ?? {},
	});
}

function summaryData(
	summary: string | Record<string, unknown>,
	options: Omit<TapeSummaryOptions, "ifChanged">,
): Record<string, unknown> {
	return compactRecord({
		scope: options.scope,
		range: options.range,
		summary: typeof summary === "string" ? summary : toPlain(summary),
		metadata: options.metadata ?? {},
	});
}

function systemEvent(
	name: string,
	data: Record<string, unknown>,
	options: TapeSystemOptions,
): TapeEntryDraft {
	return tapeRecord.event(name, data, {
		meta: systemMeta(options, options.ifChanged ? TapeChange.Changed : TapeChange.Set),
	});
}

function toIsoDate(value?: string | number | Date): string {
	if (value instanceof Date) return value.toISOString();
	if (typeof value === "number") return new Date(value).toISOString();
	if (typeof value === "string") return value;
	return new Date().toISOString();
}

function systemMeta(
	options: { scope?: string },
	change: TapeChange,
): Record<string, unknown> {
	return compactRecord({ internal: true, scope: options.scope, change });
}

function entryStatus(
	entries: readonly TapeEntry[],
	name: string,
	data: Record<string, unknown>,
): TapeConfigStatus {
	const latest = latestEventData(entries, name);
	if (!latest) return TapeStatus.Missing;
	return stableStringify(latest) === stableStringify(data) ? TapeStatus.Current : TapeStatus.Changed;
}

function latestEventData(entries: readonly TapeEntry[], name: string): unknown | null {
	for (let i = entries.length - 1; i >= 0; i--) {
		const entry = entries[i]!;
		if (entry.kind !== "event" || entry.payload.name !== name) continue;
		return entry.payload.data ?? null;
	}
	return null;
}

function serializeModel(model: unknown): Record<string, unknown> {
	const record = isPlainRecord(model) ? model : {};
	return compactRecord({
		id: record.id,
		name: record.name,
		provider: record.provider,
		api: record.api,
		reasoning: record.reasoning,
		input: record.input,
		contextWindow: record.contextWindow,
		maxTokens: record.maxTokens,
	});
}

function serializeTool(tool: unknown): Record<string, unknown> {
	const record = isPlainRecord(tool) ? tool : {};
	return compactRecord({
		name: record.name,
		label: record.label,
		description: record.description,
		parameters: toPlain(record.parameters),
	});
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function compactRecord(record: Record<string, unknown>): Record<string, unknown> {
	return Object.fromEntries(
		Object.entries(record).filter(([, value]) => value !== undefined),
	);
}

function toPlain(value: unknown): unknown {
	if (value === undefined) return undefined;
	try {
		const json = JSON.stringify(value, (_key, item) =>
			typeof item === "function" ? undefined : item,
		);
		return json === undefined ? undefined : JSON.parse(json);
	} catch {
		return String(value);
	}
}

function stableStringify(value: unknown): string {
	return JSON.stringify(sortObject(value));
}

function sortObject(value: unknown): unknown {
	if (Array.isArray(value)) return value.map(sortObject);
	if (!value || typeof value !== "object") return value;
	return Object.fromEntries(
		Object.entries(value as Record<string, unknown>)
			.sort(([a], [b]) => a.localeCompare(b))
			.map(([key, item]) => [key, sortObject(item)]),
	);
}
