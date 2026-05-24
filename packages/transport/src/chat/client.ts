/**
 * Typed client for chat methods, layered on a `RpcPeer`.
 *
 * Consumers (web frontend, CLI) own the connection lifecycle —
 * `createWebSocketConnection` → `createChatClient(conn.connection)`.
 */
import type { RpcPeer, RpcResult } from "../connection.ts";
import type {
	ChatAuthenticateParams,
	ChatCommandParams,
	ChatCommandResultParams,
	ChatListCommandsParams,
	ChatMethods,
	ChatReplyChunkParams,
	ChatReplyEndParams,
	ChatReplyParams,
	ChatSendParams,
	ChatTypingParams,
} from "./methods.ts";

export function createChatClient(peer: RpcPeer) {
	return {
		authenticate: (params: ChatAuthenticateParams) =>
			peer.request("chat/authenticate", params) as Promise<
				RpcResult<ChatMethods, "chat/authenticate">
			>,
		listCommands: (params: ChatListCommandsParams = {}) =>
			peer.request("chat/listCommands", params) as Promise<
				RpcResult<ChatMethods, "chat/listCommands">
			>,
		send: (params: ChatSendParams) => peer.notify("chat/send", params),
		command: (params: ChatCommandParams) => peer.notify("chat/command", params),
		onReply: (handler: (params: ChatReplyParams) => void) =>
			peer.on("chat/reply", handler as (params: unknown) => void),
		onTyping: (handler: (params: ChatTypingParams) => void) =>
			peer.on("chat/typing", handler as (params: unknown) => void),
		onReplyChunk: (handler: (params: ChatReplyChunkParams) => void) =>
			peer.on("chat/replyChunk", handler as (params: unknown) => void),
		onReplyEnd: (handler: (params: ChatReplyEndParams) => void) =>
			peer.on("chat/replyEnd", handler as (params: unknown) => void),
		onCommandResult: (handler: (params: ChatCommandResultParams) => void) =>
			peer.on("chat/commandResult", handler as (params: unknown) => void),
	};
}

export type ChatClient = ReturnType<typeof createChatClient>;
