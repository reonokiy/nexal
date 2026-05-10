/**
 * nexal WebSocket chat protocol — canonical wire types.
 *
 * Frames are JSON-encoded text. Both client and server share the same
 * envelope shape (`type` discriminator on every message).
 *
 * Used by:
 *   - server: `src/channels/ws.ts` (re-exports through `ws-protocol.ts`)
 *   - clients: TUI (`src/tui.ts`) and web frontend
 */

// ── Client → Server ────────────────────────────────────────────────

export interface ImageBlock {
	/** base64-encoded image bytes */
	data: string;
	/** e.g. "image/png" */
	mimeType: string;
}

export interface SendFrame {
	type: "send";
	chat_id?: string;
	sender?: string;
	text?: string;
	images?: ImageBlock[];
}

export interface CommandFrame {
	type: "command";
	chat_id?: string;
	sender?: string;
	name: string;
	args?: string[];
}

export type ClientFrame = SendFrame | CommandFrame;

// ── Server → Client ────────────────────────────────────────────────

export interface ReplyMetadata {
	worker?: {
		name: string;
		kind: string;
		lifetime: string;
	};
}

export interface ReplyFrame {
	type: "reply";
	chat_id: string;
	text: string;
	metadata?: ReplyMetadata;
}

export interface TypingFrame {
	type: "typing";
	chat_id: string;
}

export interface ReplyChunkFrame {
	type: "reply_chunk";
	chat_id: string;
	message_id: string;
	delta: string;
}

export interface ReplyEndFrame {
	type: "reply_end";
	chat_id: string;
	message_id: string;
}

export interface CommandResultFrame {
	type: "command_result";
	chat_id: string;
	name: string;
	text?: string;
	error?: string;
}

export type ServerFrame =
	| ReplyFrame
	| TypingFrame
	| ReplyChunkFrame
	| ReplyEndFrame
	| CommandResultFrame;
