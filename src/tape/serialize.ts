/**
 * Tape ↔ AgentMessage 序列化 helpers.
 *
 * Converts between pi-agent-core AgentMessages and tape.systems
 * TapeEntry payloads so we can persist conversation history and
 * rebuild it on the next session start.
 *
 * Only standard Message kinds are round-tripped:
 *   user       → TapeEntry(kind="message", payload={role,content,timestamp})
 *   assistant  → TapeEntry(kind="message", payload={role,content,api,provider,model,usage,stopReason,timestamp})
 *   toolResult → TapeEntry(kind="tool_result", payload={role,toolCallId,toolName,content,details,isError,timestamp})
 *
 * CustomAgentMessages and transient events are dropped during rebuild.
 */
import type { AgentMessage } from "@mariozechner/pi-agent-core";
import type { TextContent, ImageContent, UserMessage, AssistantMessage, ToolResultMessage } from "@mariozechner/pi-ai";
import type { TapeEntry } from "./types.ts";

const MAX_CONTEXT_MESSAGES = 200; // safety cap for prototype

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

// ── content serialization ──────────────────────────────────────────

/** Normalize UserMessage content for JSON storage. */
function serializeContent(content: UserMessage["content"]): unknown {
	if (typeof content === "string") return content;
	return content.map((block) => {
		if (block.type === "text") return { type: "text", text: block.text };
		if (block.type === "image") return { type: "image", data: block.data, mimeType: block.mimeType };
		return block;
	});
}

/** Restore UserMessage content from JSON. */
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

/** Normalize AssistantMessage content (text | thinking | toolCall). */
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

// ── context window helpers ─────────────────────────────────────────

/**
 * Truncate message list to MAX_CONTEXT_MESSAGES if it exceeds the budget.
 * Keeps the most recent messages (tail).
 */
export function truncateMessages(messages: AgentMessage[]): AgentMessage[] {
	if (messages.length <= MAX_CONTEXT_MESSAGES) return messages;
	return messages.slice(-MAX_CONTEXT_MESSAGES);
}
