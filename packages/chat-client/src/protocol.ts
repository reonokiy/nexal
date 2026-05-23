/**
 * nexal WebSocket chat protocol — canonical wire types.
 *
 * Frames are msgpack-encoded binary. Both client and server share the
 * same envelope shape (`type` discriminator on every message).
 *
 * Used by:
 *   - server: `src/channels/ws.ts`
 *   - clients: web frontend
 */

// ── Constants ──────────────────────────────────────────────────────

export const ClientFrameType = {
	Auth: "auth",
	Send: "send",
	Command: "command",
	ListCommands: "list_commands",
} as const;

export const ServerFrameType = {
	Reply: "reply",
	Typing: "typing",
	ReplyChunk: "reply_chunk",
	ReplyEnd: "reply_end",
	CommandResult: "command_result",
	ListCommandsResult: "list_commands_result",
	AuthOk: "auth_ok",
	AuthError: "auth_error",
} as const;

export type ClientFrameType = (typeof ClientFrameType)[keyof typeof ClientFrameType];
export type ServerFrameType = (typeof ServerFrameType)[keyof typeof ServerFrameType];

// ── Command metadata ───────────────────────────────────────────────

export interface CommandInfo {
	name: string;
	description: string;
}

// ── Client → Server ────────────────────────────────────────────────

export interface ImageBlock {
	/** Raw image bytes */
	data: Uint8Array;
	/** e.g. "image/png" */
	mimeType: string;
}

export interface AuthFrame {
	type: typeof ClientFrameType.Auth;
	token: string;
}

export interface SendFrame {
	type: typeof ClientFrameType.Send;
	chat_id?: string;
	sender?: string;
	text?: string;
	images?: ImageBlock[];
}

export interface CommandFrame {
	type: typeof ClientFrameType.Command;
	chat_id?: string;
	sender?: string;
	name: string;
	args?: string[];
}

export interface ListCommandsFrame {
	type: typeof ClientFrameType.ListCommands;
}

export type ClientFrame = AuthFrame | SendFrame | CommandFrame | ListCommandsFrame;

// ── Server → Client ────────────────────────────────────────────────

export interface ReplyMetadata {
	worker?: {
		name: string;
		kind: string;
		lifetime: string;
	};
}

export interface ReplyFrame {
	type: typeof ServerFrameType.Reply;
	chat_id: string;
	text: string;
	metadata?: ReplyMetadata;
}

export interface TypingFrame {
	type: typeof ServerFrameType.Typing;
	chat_id: string;
}

export interface ReplyChunkFrame {
	type: typeof ServerFrameType.ReplyChunk;
	chat_id: string;
	message_id: string;
	delta: string;
}

export interface ReplyEndFrame {
	type: typeof ServerFrameType.ReplyEnd;
	chat_id: string;
	message_id: string;
}

export interface CommandResultFrame {
	type: typeof ServerFrameType.CommandResult;
	chat_id: string;
	name: string;
	text?: string;
	error?: string;
	/** Optional structured payload for UI consumers (settings page etc.). */
	data?: unknown;
}

export interface ListCommandsResultFrame {
	type: typeof ServerFrameType.ListCommandsResult;
	commands: CommandInfo[];
}

export interface AuthResultFrame {
	type: typeof ServerFrameType.AuthOk;
	user_id: string;
	email?: string;
}

export interface AuthErrorFrame {
	type: typeof ServerFrameType.AuthError;
	error: string;
}

export type ServerFrame =
	| ReplyFrame
	| TypingFrame
	| ReplyChunkFrame
	| ReplyEndFrame
	| CommandResultFrame
	| ListCommandsResultFrame
	| AuthResultFrame
	| AuthErrorFrame;
