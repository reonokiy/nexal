import type { AgentTool } from "@mariozechner/pi-agent-core";
import type { Model } from "@mariozechner/pi-ai";
import { tapeRecord, type TapeEntry, type TapeEntryDraft } from "@nexal/tape";

export const RUNTIME_CONTEXT_EVENT = "runtime/context";

export interface RuntimeContextInput {
	scope: "session" | "worker";
	systemPrompt: string;
	model: Model<any>;
	tools: AgentTool<any>[];
	metadata?: Record<string, unknown>;
}

export type RuntimeContextStatus = "missing" | "current" | "changed";

export function hasRuntimeContextEntry(entries: readonly TapeEntry[]): boolean {
	return entries.some((entry) =>
		entry.kind === "event" &&
		entry.payload.name === RUNTIME_CONTEXT_EVENT
	);
}

export function runtimeContextStatus(
	entries: readonly TapeEntry[],
	input: RuntimeContextInput,
): RuntimeContextStatus {
	const latest = latestRuntimeContextData(entries);
	if (!latest) return "missing";
	return stableStringify(latest) === stableStringify(runtimeContextData(input))
		? "current"
		: "changed";
}

export function runtimeContextRecord(
	input: RuntimeContextInput,
	options: { change?: Exclude<RuntimeContextStatus, "current"> } = {},
): TapeEntryDraft {
	return tapeRecord.event(
		RUNTIME_CONTEXT_EVENT,
		runtimeContextData(input),
		{ meta: { internal: true, scope: input.scope, change: options.change ?? "created" } },
	);
}

function runtimeContextData(input: RuntimeContextInput): Record<string, unknown> {
	return {
		scope: input.scope,
		systemPrompt: input.systemPrompt,
		model: serializeModel(input.model),
		tools: input.tools.map(serializeTool),
		metadata: input.metadata ?? {},
	};
}

function latestRuntimeContextData(entries: readonly TapeEntry[]): unknown | null {
	for (let i = entries.length - 1; i >= 0; i--) {
		const entry = entries[i]!;
		if (entry.kind !== "event" || entry.payload.name !== RUNTIME_CONTEXT_EVENT) continue;
		return entry.payload.data ?? null;
	}
	return null;
}

function serializeModel(model: Model<any>): Record<string, unknown> {
	const record = model as unknown as Record<string, unknown>;
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

function serializeTool(tool: AgentTool<any>): Record<string, unknown> {
	return compactRecord({
		name: tool.name,
		label: tool.label,
		description: tool.description,
		parameters: toPlain((tool as unknown as Record<string, unknown>).parameters),
	});
}

function compactRecord(record: Record<string, unknown>): Record<string, unknown> {
	return Object.fromEntries(
		Object.entries(record).filter(([, value]) => value !== undefined),
	);
}

function toPlain(value: unknown): unknown {
	try {
		return JSON.parse(JSON.stringify(value, (_key, item) =>
			typeof item === "function" ? undefined : item,
		));
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
