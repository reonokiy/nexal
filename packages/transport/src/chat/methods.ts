/**
 * Chat methods — frontend ↔ backend chat WebSocket protocol.
 *
 * The browser/CLI client speaks to the `ws` channel using the same
 * RPC envelope as `agent.ts` / `gateway.ts`: msgpack-binary frames
 * with `{id, method, params}` / `{id, result|error}` / `{method, params}`.
 *
 * Request methods (client → server, expects response):
 *   - chat/authenticate
 *   - chat/listCommands
 *
 * Notification methods:
 *   - client → server: chat/send, chat/command
 *   - server → client: chat/reply, chat/typing, chat/replyChunk,
 *                      chat/replyEnd, chat/commandResult
 */

// ── Shared payloads ──────────────────────────────────────────────────

export interface ImageBlock {
	/** Raw image bytes. */
	data: Uint8Array;
	/** e.g. "image/png". */
	mimeType: string;
}

export interface CommandInfo {
	name: string;
	description: string;
}

export interface ReplyMetadata {
	worker?: {
		name: string;
		kind: string;
		lifetime: string;
	};
}

// ── Requests ─────────────────────────────────────────────────────────

export interface ChatAuthenticateParams {
	token: string;
}
export interface ChatAuthenticateResult {
	userId: string;
	email?: string;
}

export type ChatListCommandsParams = Record<string, never>;
export interface ChatListCommandsResult {
	commands: CommandInfo[];
}

/** Discriminated map for typed RPC dispatch. */
export interface ChatMethods {
	"chat/authenticate": {
		params: ChatAuthenticateParams;
		result: ChatAuthenticateResult;
	};
	"chat/listCommands": {
		params: ChatListCommandsParams;
		result: ChatListCommandsResult;
	};
}

// ── Notifications ────────────────────────────────────────────────────

// client → server

export interface ChatSendParams {
	chatId?: string;
	sender?: string;
	text?: string;
	images?: ImageBlock[];
}

export interface ChatCommandParams {
	chatId?: string;
	sender?: string;
	name: string;
	args?: string[];
}

// server → client

export interface ChatReplyParams {
	chatId: string;
	text: string;
	metadata?: ReplyMetadata;
}

export interface ChatTypingParams {
	chatId: string;
}

export interface ChatReplyChunkParams {
	chatId: string;
	messageId: string;
	delta: string;
}

export interface ChatReplyEndParams {
	chatId: string;
	messageId: string;
}

export interface ChatCommandResultParams {
	chatId: string;
	name: string;
	text?: string;
	error?: string;
	/** Optional structured payload for UI consumers (settings page etc.). */
	data?: unknown;
}

export interface ChatNotifications {
	"chat/send": ChatSendParams;
	"chat/command": ChatCommandParams;
	"chat/reply": ChatReplyParams;
	"chat/typing": ChatTypingParams;
	"chat/replyChunk": ChatReplyChunkParams;
	"chat/replyEnd": ChatReplyEndParams;
	"chat/commandResult": ChatCommandResultParams;
}
