import type { TapeEntryDraft, TapeRef } from "./types.ts";

export interface TapeRecordOptions {
	meta?: Record<string, unknown>;
	date?: string | number | Date;
}

export interface TapeMessagePayload extends Record<string, unknown> {
	role: string;
	content?: unknown;
	timestamp?: number;
}

export interface AssistantMessageRecordOptions extends TapeRecordOptions {
	api?: string;
	provider?: string;
	model?: string;
	responseId?: string;
	usage?: unknown;
	stopReason?: string;
	errorMessage?: string;
	timestamp?: number;
}

export interface ToolResultRecordInput {
	toolCallId: string;
	toolName: string;
	content?: unknown;
	details?: unknown;
	isError?: boolean;
	timestamp?: number;
}

export const tapeRecord = {
	anchor(name: string, state?: Record<string, unknown>, options: TapeRecordOptions = {}): TapeEntryDraft {
		const payload: Record<string, unknown> = { name };
		if (state) payload.state = state;
		return draft("anchor", payload, options);
	},

	message(payload: TapeMessagePayload, options: TapeRecordOptions = {}): TapeEntryDraft {
		return draft("message", payload, options, payload.timestamp);
	},

	userMessage(content: unknown, options: TapeRecordOptions & { timestamp?: number } = {}): TapeEntryDraft {
		const timestamp = options.timestamp ?? Date.now();
		return draft(
			"message",
			{ role: "user", content, timestamp },
			options,
			timestamp,
		);
	},

	assistantMessage(content: unknown, options: AssistantMessageRecordOptions = {}): TapeEntryDraft {
		const timestamp = options.timestamp ?? Date.now();
		return draft(
			"message",
			{
				role: "assistant",
				content,
				api: options.api ?? "",
				provider: options.provider ?? "",
				model: options.model ?? "",
				responseId: options.responseId,
				usage: options.usage,
				stopReason: options.stopReason ?? "stop",
				errorMessage: options.errorMessage,
				timestamp,
			},
			options,
			timestamp,
		);
	},

	toolResult(input: ToolResultRecordInput, options: TapeRecordOptions = {}): TapeEntryDraft {
		const timestamp = input.timestamp ?? Date.now();
		return draft(
			"tool_result",
			{
				role: "toolResult",
				toolCallId: input.toolCallId,
				toolName: input.toolName,
				content: input.content ?? [],
				details: input.details,
				isError: input.isError ?? false,
				timestamp,
			},
			options,
			timestamp,
		);
	},

	event(name: string, data?: unknown, options: TapeRecordOptions = {}): TapeEntryDraft {
		return draft("event", { name, data }, options);
	},

	ref(ref: TapeRef, options: TapeRecordOptions = {}): TapeEntryDraft {
		return draft("ref", { ref }, options);
	},

	redaction(targetId: number, reason?: string, options: TapeRecordOptions = {}): TapeEntryDraft {
		return draft("redaction", { targetId, reason, redactedAt: Date.now() }, options);
	},

	amendment(
		targetIds: number[],
		replacement: TapeEntryDraft[],
		reason?: string,
		options: TapeRecordOptions = {},
	): TapeEntryDraft {
		return draft("amendment", { targetIds, replacement, reason, amendedAt: Date.now() }, options);
	},
};

function draft(
	kind: TapeEntryDraft["kind"],
	payload: Record<string, unknown>,
	options: TapeRecordOptions,
	timestamp?: number,
): TapeEntryDraft {
	return {
		kind,
		payload,
		meta: options.meta ?? {},
		date: toIsoDate(options.date, timestamp),
	};
}

function toIsoDate(value?: string | number | Date, timestamp?: number): string {
	if (value instanceof Date) return value.toISOString();
	if (typeof value === "number") return new Date(value).toISOString();
	if (typeof value === "string") return value;
	return new Date(timestamp ?? Date.now()).toISOString();
}
