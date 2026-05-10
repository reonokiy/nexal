/**
 * WS channel wire protocol.
 *
 * Canonical types live in `@nexal/chat-client`; this file re-exports
 * them under the legacy `Ws*` names so the existing server-side
 * imports keep working.
 */
import type {
	ClientFrame,
	CommandFrame,
	CommandResultFrame,
	ImageBlock,
	ReplyChunkFrame,
	ReplyEndFrame,
	ReplyFrame,
	SendFrame,
	ServerFrame,
	TypingFrame,
} from "@nexal/chat-client";

export type WsImageBlock = ImageBlock;
export type WsSendFrame = SendFrame;
export type WsCommandFrame = CommandFrame;
export type WsClientFrame = ClientFrame;
export type WsReplyFrame = ReplyFrame;
export type WsTypingFrame = TypingFrame;
export type WsReplyChunkFrame = ReplyChunkFrame;
export type WsReplyEndFrame = ReplyEndFrame;
export type WsCommandResultFrame = CommandResultFrame;
export type WsServerFrame = ServerFrame;
