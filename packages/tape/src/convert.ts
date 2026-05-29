/**
 * Tape ↔ message conversion layer.
 *
 * Tape is the canonical memory format; consumers convert to model/agent
 * message objects only at the model boundary. The message shapes here are
 * intentionally structural so @nexal/tape stays independent from any
 * concrete agent or LLM SDK package.
 */
import type { TapeEntry } from "./types.ts";

export interface TextContent {
	type: "text";
	text: string;
	textSignature?: string;
}

export interface ImageContent {
	type: "image";
	data: unknown;
	mimeType: string;
}

export interface TapeMessage {
	role: string;
	content?: unknown;
	timestamp?: number;
	meta?: Record<string, unknown>;
}

export type UserMessage = TapeMessage & { role: "user" };
export type AssistantMessage = TapeMessage & { role: "assistant" };
export type ToolResultMessage = TapeMessage & { role: "toolResult" };

const MAX_CONTEXT_MESSAGES = 200;
const BYTES_MARKER = "__nexal_bytes_b64__";

// ── TapeEntry → message ───────────────────────────────────────────

/** Convert a list of tape entries → messages (skips non-message kinds). */
export function entriesToMessages(entries: readonly TapeEntry[]): TapeMessage[] {
	const out: TapeMessage[] = [];
	for (const e of entries) {
		const m = entryToMessage(e);
		if (m) out.push(m);
	}
	return out;
}

/** Convert a single tape entry → message (or null if not a message kind). */
function entryToMessage(entry: TapeEntry): TapeMessage | null {
	const p = entry.payload;
	switch (entry.kind) {
		case "message": {
			const role = p.role as string;
			if (role === "user") {
				return withEntryMeta({
					role: "user",
					content: reviveContent(p.content),
					timestamp: Number(p.timestamp ?? Date.now()),
				}, entry);
			}
			if (role === "assistant") {
				return withEntryMeta({
					role: "assistant",
					content: reviveAssistantContent(p.content),
					api: String(p.api ?? ""),
					provider: String(p.provider ?? ""),
					model: String(p.model ?? ""),
					responseId: p.responseId ? String(p.responseId) : undefined,
					usage: p.usage ?? {
						input: 0,
						output: 0,
						cacheRead: 0,
						cacheWrite: 0,
						totalTokens: 0,
						cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
					},
					stopReason: p.stopReason ?? "stop",
					errorMessage: p.errorMessage ? String(p.errorMessage) : undefined,
					timestamp: Number(p.timestamp ?? Date.now()),
				}, entry);
			}
			return null;
		}
		case "tool_result": {
			return withEntryMeta({
				role: "toolResult",
				toolCallId: String(p.toolCallId ?? ""),
				toolName: String(p.toolName ?? ""),
				content: reviveContent(p.content) as Array<TextContent | ImageContent>,
				details: p.details,
				isError: Boolean(p.isError),
				timestamp: Number(p.timestamp ?? Date.now()),
			}, entry);
		}
		default:
			return null;
	}
}

function withEntryMeta<T extends TapeMessage>(message: T, entry: TapeEntry): T {
	return Object.keys(entry.meta).length > 0
		? { ...message, meta: entry.meta } as T
		: message;
}

// ── message → TapeEntry ───────────────────────────────────────────

/** Convert messages → tape entries (only standard kinds). */
export function messagesToEntries(messages: readonly unknown[]): Omit<TapeEntry, "id" | "date">[] {
	return messages.map((m) => messageToEntry(m)).filter(Boolean) as Omit<TapeEntry, "id" | "date">[];
}

function messageToEntry(msg: unknown): Omit<TapeEntry, "id" | "date"> | null {
	if (!msg || typeof msg !== "object") return null;
	const role = (msg as Record<string, unknown>).role as string;
	if (role === "user") {
		const um = msg as UserMessage;
		return {
			kind: "message",
			payload: {
				role: "user",
				content: serializeContent(um.content),
				timestamp: um.timestamp,
			},
			meta: messageMeta(msg),
		};
	}
	if (role === "assistant") {
		const am = msg as AssistantMessage & Record<string, unknown>;
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
			meta: messageMeta(msg),
		};
	}
	if (role === "toolResult") {
		const tr = msg as ToolResultMessage & Record<string, unknown>;
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
			meta: messageMeta(msg),
		};
	}
	return null;
}

function messageMeta(msg: unknown): Record<string, unknown> {
	const meta = (msg as { meta?: unknown }).meta;
	return isPlainRecord(meta) ? meta : {};
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

// ── message ↔ JSON ────────────────────────────────────────────────

/** Serialize messages to JSON, handling Uint8Array via base64. */
export function messagesToJson(msgs: readonly unknown[]): string {
	return JSON.stringify(msgs, (_key, value) => {
		if (value instanceof Uint8Array) {
			return { [BYTES_MARKER]: bytesToBase64(value) };
		}
		return value;
	});
}

/** Deserialize JSON back to messages, restoring base64-encoded Uint8Arrays. */
export function jsonToMessages<T = TapeMessage>(s: string): T[] {
	if (!s || s === "[]") return [];
	return JSON.parse(s, (_key, value) => {
		if (value && typeof value === "object" && typeof (value as Record<string, unknown>)[BYTES_MARKER] === "string") {
			return base64ToBytes(String((value as Record<string, unknown>)[BYTES_MARKER]));
		}
		return value;
	}) as T[];
}

function bytesToBase64(bytes: Uint8Array): string {
	const buffer = (globalThis as { Buffer?: { from(input: Uint8Array): { toString(encoding: "base64"): string } } }).Buffer;
	if (buffer) return buffer.from(bytes).toString("base64");
	if (typeof btoa !== "function") throw new Error("No base64 encoder available in this runtime");
	let binary = "";
	for (const byte of bytes) binary += String.fromCharCode(byte);
	return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
	const buffer = (globalThis as { Buffer?: { from(input: string, encoding: "base64"): Uint8Array } }).Buffer;
	if (buffer) return new Uint8Array(buffer.from(value, "base64"));
	if (typeof atob !== "function") throw new Error("No base64 decoder available in this runtime");
	const binary = atob(value);
	const bytes = new Uint8Array(binary.length);
	for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
	return bytes;
}

// ── content serialization ──────────────────────────────────────────

function serializeContent(content: unknown): unknown {
	if (typeof content === "string") return content;
	if (!Array.isArray(content)) return content;
	return content.map((block) => {
		const record = isPlainRecord(block) ? block : null;
		if (record?.type === "text") return { type: "text", text: record.text };
		if (record?.type === "image") return { type: "image", data: record.data, mimeType: record.mimeType };
		return block;
	});
}

function reviveContent(raw: unknown): UserMessage["content"] {
	if (typeof raw === "string") return raw;
	if (!Array.isArray(raw)) return String(raw);
	return raw.map((block: unknown) => {
		const record = isPlainRecord(block) ? block : null;
		if (record?.type === "text") return { type: "text", text: String(record.text ?? "") } as TextContent;
		if (record?.type === "image") {
			return {
				type: "image",
				data: String(record.data ?? ""),
				mimeType: String(record.mimeType ?? "image/png"),
			} as ImageContent;
		}
		return { type: "text", text: String(block) } as TextContent;
	});
}

function serializeAssistantContent(content: unknown): unknown {
	if (typeof content === "string") return [{ type: "text", text: content }];
	if (!Array.isArray(content)) return [{ type: "text", text: String(content ?? "") }];
	return content.map((block) => {
		const record = isPlainRecord(block) ? block : null;
		if (record?.type === "text") return { type: "text", text: record.text, textSignature: record.textSignature };
		if (record?.type === "thinking") return { type: "thinking", thinking: record.thinking, thinkingSignature: record.thinkingSignature, redacted: record.redacted };
		if (record?.type === "toolCall") return { type: "toolCall", id: record.id, name: record.name, arguments: record.arguments, thoughtSignature: record.thoughtSignature };
		return block;
	});
}

function reviveAssistantContent(raw: unknown): AssistantMessage["content"] {
	if (!Array.isArray(raw)) return [{ type: "text", text: String(raw ?? "") }];
	return raw.map((block: unknown) => {
		const record = isPlainRecord(block) ? block : null;
		switch (record?.type) {
			case "text":
				return { type: "text", text: String(record.text ?? ""), textSignature: record.textSignature };
			case "thinking":
				return {
					type: "thinking",
					thinking: String(record.thinking ?? ""),
					thinkingSignature: record.thinkingSignature,
					redacted: record.redacted,
				};
			case "toolCall":
				return {
					type: "toolCall",
					id: String(record.id ?? ""),
					name: String(record.name ?? ""),
					arguments: record.arguments ?? {},
					thoughtSignature: record.thoughtSignature,
				};
			default:
				return { type: "text", text: String(block) };
		}
	});
}

// ── TapeEntry → LLM Message ────────────────────────────────────────

/**
 * Convert TapeEntry[] to model-boundary messages.
 * Alias of entriesToMessages kept for call-site readability.
 */
export function entriesToLlmMessages(entries: readonly TapeEntry[]): TapeMessage[] {
	return entriesToMessages(entries);
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
export function truncateMessages(messages: TapeMessage[]): TapeMessage[] {
	if (messages.length <= MAX_CONTEXT_MESSAGES) return messages;
	return messages.slice(-MAX_CONTEXT_MESSAGES);
}
