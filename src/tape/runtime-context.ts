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

export function hasRuntimeContextEntry(entries: readonly TapeEntry[]): boolean {
	return entries.some((entry) =>
		entry.kind === "event" &&
		entry.payload.name === RUNTIME_CONTEXT_EVENT
	);
}

export function runtimeContextRecord(input: RuntimeContextInput): TapeEntryDraft {
	return tapeRecord.event(
		RUNTIME_CONTEXT_EVENT,
		{
			scope: input.scope,
			systemPrompt: input.systemPrompt,
			model: serializeModel(input.model),
			tools: input.tools.map(serializeTool),
			metadata: input.metadata ?? {},
		},
		{ meta: { internal: true, scope: input.scope } },
	);
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
