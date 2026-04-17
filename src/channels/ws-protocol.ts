/**
 * WS channel wire protocol — typed frames between TUI/clients and the
 * nexal WS server.
 *
 * Client → Server:
 *   WsSendFrame        — regular chat message
 *   WsCommandFrame     — slash command invocation
 *
 * Server → Client:
 *   WsReplyFrame       — agent/coordinator reply
 *   WsTypingFrame      — typing indicator
 *   WsCommandResultFrame — slash command result
 */

// ── Client → Server ────────────────────────────────────────────────

export interface WsImageBlock {
	data: string;       // base64-encoded image bytes
	mimeType: string;   // e.g. "image/png"
}

export interface WsSendFrame {
	type: "send";
	chat_id?: string;
	sender?: string;
	text?: string;
	images?: WsImageBlock[];
}

export interface WsCommandFrame {
	type: "command";
	chat_id?: string;
	sender?: string;
	name: string;
	args?: string[];
}

export type WsClientFrame = WsSendFrame | WsCommandFrame;

// ── Server → Client ────────────────────────────────────────────────

export interface WsReplyFrame {
	type: "reply";
	chat_id: string;
	text: string;
	metadata?: {
		worker?: {
			name: string;
			kind: string;
			lifetime: string;
		};
	};
}

export interface WsTypingFrame {
	type: "typing";
	chat_id: string;
}

export interface WsCommandResultFrame {
	type: "command_result";
	chat_id: string;
	name: string;
	text?: string;
	error?: string;
}

export type WsServerFrame = WsReplyFrame | WsTypingFrame | WsCommandResultFrame;
