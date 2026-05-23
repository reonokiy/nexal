/**
 * Tape ↔ AgentMessage conversion layer.
 *
 * Single source of truth for converting between tape TapeEntry format
 * and pi-agent-core AgentMessage format. Also provides JSON serialization
 * for the worker store's `messages_json` column.
 *
 * Conversion to AgentMessage should ONLY happen when interacting with
 * the LLM model. Tape (TapeEntry) is the canonical memory format.
 */
import type { AgentMessage } from "@mariozechner/pi-agent-core";
import type { Message, TextContent, ImageContent, UserMessage, AssistantMessage, ToolResultMessage } from "@mariozechner/pi-ai";
import type { TapeEntry } from "@nexal/tape";

const MAX_CONTEXT_MESSAGES = 200;
const BYTES_MARKER = "__nexal_bytes_b64__";

// ── TapeEntry → AgentMessage ──────────────────────────────────────

/** Convert a list of tape entries → AgentMessages (skips non-message kinds). */
export function entriesToMessages(entries: TapeEntry[]): AgentMessage[] {
	const out: AgentMessage[] = [];
	for (const e of entries) {
		const m = entryToMessage(e);
		if (m) out.push(m);
	}
	return out;
}

/** Convert a single tape entry → AgentMessage (or null if not a message kind). */
function entryToMessage(entry: TapeEntry): AgentMessage | null {
	const p = entry.payload;
	switch (entry.kind) {
		case "message": {
			const role = p.role as string;
			if (role === "user") {
				return {
					role: "user",
					content: reviveContent(p.content),
					timestamp: Number(p.timestamp ?? Date.now()),
				} satisfies UserMessage;
			}
			if (role === "assistant") {
				return {
					role: "assistant",
					content: reviveAssistantContent(p.content),
					api: String(p.api ?? ""),
					provider: String(p.provider ?? ""),
					model: String(p.model ?? ""),
					responseId: p.responseId ? String(p.responseId) : undefined,
					usage: (p.usage as any) ?? {
						input: 0,
						output: 0,
						cacheRead: 0,
						cacheWrite: 0,
						totalTokens: 0,
						cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
					},
					stopReason: (p.stopReason as any) ?? "stop",
					errorMessage: p.errorMessage ? String(p.errorMessage) : undefined,
					timestamp: Number(p.timestamp ?? Date.now()),
				} satisfies AssistantMessage;
			}
			return null;
		}
		case "tool_result": {
			return {
				role: "toolResult",
				toolCallId: String(p.toolCallId ?? ""),
				toolName: String(p.toolName ?? ""),
				content: reviveContent(p.content) as (TextContent | ImageContent)[],
				details: p.details,
				isError: Boolean(p.isError),
				timestamp: Number(p.timestamp ?? Date.now()),
			} satisfies ToolResultMessage;
		}
		default:
			return null;
	}
}

// ── AgentMessage → TapeEntry ──────────────────────────────────────

/** Convert AgentMessages → tape entries (only standard kinds). */
export function messagesToEntries(messages: AgentMessage[]): Omit<TapeEntry, "id" | "date">[] {
	return messages.map((m) => messageToEntry(m)).filter(Boolean) as Omit<TapeEntry, "id" | "date">[];
}

function messageToEntry(msg: AgentMessage): Omit<TapeEntry, "id" | "date"> | null {
	if (!msg || typeof msg !== "object") return null;
	const role = (msg as any).role as string;
	if (role === "user") {
		const um = msg as UserMessage;
		return {
			kind: "message",
			payload: {
				role: "user",
				content: serializeContent(um.content),
				timestamp: um.timestamp,
			},
			meta: {},
		};
	}
	if (role === "assistant") {
		const am = msg as AssistantMessage;
		return {
			kind: "message",
			payload: {
				role: "assistant",
				content: serializeAssistantContent(am.content),
				api: am.api,
				provider: am.provider,
				model: am.model,
				responseId: am.responseId,
				usage: am.usage,
				stopReason: am.stopReason,
				errorMessage: am.errorMessage,
				timestamp: am.timestamp,
			},
			meta: {},
		};
	}
	if (role === "toolResult") {
		const tr = msg as ToolResultMessage;
		return {
			kind: "tool_result",
			payload: {
				role: "toolResult",
				toolCallId: tr.toolCallId,
				toolName: tr.toolName,
				content: serializeContent(tr.content),
				details: tr.details,
				isError: tr.isError,
				timestamp: tr.timestamp,
			},
			meta: {},
		};
	}
	return null;
}

// ── AgentMessage ↔ JSON (for worker store messages_json column) ──

/** Serialize AgentMessages to JSON, handling Uint8Array via base64. */
export function messagesToJson(msgs: AgentMessage[]): string {
	return JSON.stringify(msgs, (_key, value) => {
		if (value instanceof Uint8Array) {
			return { [BYTES_MARKER]: Buffer.from(value).toString("base64") };
		}
		return value;
	});
}

/** Deserialize JSON back to AgentMessages, restoring base64-encoded Uint8Arrays. */
export function jsonToMessages(s: string): AgentMessage[] {
	if (!s || s === "[]") return [];
	return JSON.parse(s, (_key, value) => {
		if (value && typeof value === "object" && typeof (value as any)[BYTES_MARKER] === "string") {
			return new Uint8Array(Buffer.from((value as any)[BYTES_MARKER], "base64"));
		}
		return value;
	}) as AgentMessage[];
}

// ── content serialization ──────────────────────────────────────────

function serializeContent(content: UserMessage["content"]): unknown {
	if (typeof content === "string") return content;
	return content.map((block) => {
		if (block.type === "text") return { type: "text", text: block.text };
		if (block.type === "image") return { type: "image", data: block.data, mimeType: block.mimeType };
		return block;
	});
}

function reviveContent(raw: unknown): UserMessage["content"] {
	if (typeof raw === "string") return raw;
	if (!Array.isArray(raw)) return String(raw);
	return raw.map((block: any) => {
		if (block?.type === "text") return { type: "text", text: String(block.text ?? "") } as TextContent;
		if (block?.type === "image") {
			return {
				type: "image",
				data: String(block.data ?? ""),
				mimeType: String(block.mimeType ?? "image/png"),
			} as ImageContent;
		}
		return { type: "text", text: String(block) } as TextContent;
	});
}

function serializeAssistantContent(content: AssistantMessage["content"]): unknown {
	return content.map((block) => {
		if (block.type === "text") return { type: "text", text: block.text, textSignature: block.textSignature };
		if (block.type === "thinking") return { type: "thinking", thinking: block.thinking, thinkingSignature: block.thinkingSignature, redacted: block.redacted };
		if (block.type === "toolCall") return { type: "toolCall", id: block.id, name: block.name, arguments: block.arguments, thoughtSignature: block.thoughtSignature };
		return block;
	});
}

function reviveAssistantContent(raw: unknown): AssistantMessage["content"] {
	if (!Array.isArray(raw)) return [{ type: "text", text: String(raw ?? "") }];
	return raw.map((block: any) => {
		switch (block?.type) {
			case "text":
				return { type: "text", text: String(block.text ?? ""), textSignature: block.textSignature };
			case "thinking":
				return {
					type: "thinking",
					thinking: String(block.thinking ?? ""),
					thinkingSignature: block.thinkingSignature,
					redacted: block.redacted,
				};
			case "toolCall":
				return {
					type: "toolCall",
					id: String(block.id ?? ""),
					name: String(block.name ?? ""),
					arguments: block.arguments ?? {},
					thoughtSignature: block.thoughtSignature,
				};
			default:
				return { type: "text", text: String(block) };
		}
	});
}

// ── TapeEntry → LLM Message (direct, no AgentMessage intermediate) ──

/**
 * Convert TapeEntry[] directly to LLM Message[] for model interaction.
 * This is the preferred path — tape is the canonical format, and we
 * convert to LLM format only at the model boundary.
 */
export function entriesToLlmMessages(entries: TapeEntry[]): Message[] {
	const out: Message[] = [];
	for (const e of entries) {
		const m = entryToLlmMessage(e);
		if (m) out.push(m);
	}
	return out;
}

function entryToLlmMessage(entry: TapeEntry): Message | null {
	const p = entry.payload;
	switch (entry.kind) {
		case "message": {
			const role = p.role as string;
			if (role === "user") {
				return {
					role: "user",
					content: reviveContent(p.content),
					timestamp: Number(p.timestamp ?? Date.now()),
				} satisfies UserMessage;
			}
			if (role === "assistant") {
				return {
					role: "assistant",
					content: reviveAssistantContent(p.content),
					api: String(p.api ?? ""),
					provider: String(p.provider ?? ""),
					model: String(p.model ?? ""),
					responseId: p.responseId ? String(p.responseId) : undefined,
					usage: (p.usage as any) ?? {
						input: 0,
						output: 0,
						cacheRead: 0,
						cacheWrite: 0,
						totalTokens: 0,
						cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
					},
					stopReason: (p.stopReason as any) ?? "stop",
					errorMessage: p.errorMessage ? String(p.errorMessage) : undefined,
					timestamp: Number(p.timestamp ?? Date.now()),
				} satisfies AssistantMessage;
			}
			return null;
		}
		case "tool_result": {
			return {
				role: "toolResult",
				toolCallId: String(p.toolCallId ?? ""),
				toolName: String(p.toolName ?? ""),
				content: reviveContent(p.content) as (TextContent | ImageContent)[],
				details: p.details,
				isError: Boolean(p.isError),
				timestamp: Number(p.timestamp ?? Date.now()),
			} satisfies ToolResultMessage;
		}
		default:
			return null;
	}
}

// ── context window helpers ─────────────────────────────────────────

/**
 * Truncate tape entries to MAX_CONTEXT_MESSAGES if it exceeds the budget.
 * Keeps the most recent entries (tail).
 */
export function truncateEntries(entries: TapeEntry[]): TapeEntry[] {
	if (entries.length <= MAX_CONTEXT_MESSAGES) return entries;
	return entries.slice(-MAX_CONTEXT_MESSAGES);
}

/**
 * Truncate message list to MAX_CONTEXT_MESSAGES if it exceeds the budget.
 * Keeps the most recent messages (tail).
 * @deprecated Use truncateEntries for tape-native code.
 */
export function truncateMessages(messages: AgentMessage[]): AgentMessage[] {
	if (messages.length <= MAX_CONTEXT_MESSAGES) return messages;
	return messages.slice(-MAX_CONTEXT_MESSAGES);
}
