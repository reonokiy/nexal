/**
 * Typed handlers for chat methods. Requests are registered via
 * `handleChatRequests`; the asymmetric client→server notification
 * stream (`send`, `command`) goes through `handleChatNotifications`.
 */
import type { NotifyPeer, RequestPeer } from "../connection.ts";
import type {
	ChatAuthenticateParams,
	ChatAuthenticateResult,
	ChatCommandParams,
	ChatListCommandsParams,
	ChatListCommandsResult,
	ChatSendParams,
} from "./methods.ts";

export interface ChatRequestHandlers {
	authenticate?: (
		params: ChatAuthenticateParams,
	) => ChatAuthenticateResult | Promise<ChatAuthenticateResult>;
	listCommands?: (
		params: ChatListCommandsParams,
	) => ChatListCommandsResult | Promise<ChatListCommandsResult>;
}

export interface ChatNotificationHandlers {
	send?: (params: ChatSendParams) => void;
	command?: (params: ChatCommandParams) => void;
}

export function handleChatRequests(peer: RequestPeer, handlers: ChatRequestHandlers): void {
	if (handlers.authenticate)
		peer.handleRequest(
			"chat/authenticate",
			handlers.authenticate as (params: unknown) => unknown,
		);
	if (handlers.listCommands)
		peer.handleRequest(
			"chat/listCommands",
			handlers.listCommands as (params: unknown) => unknown,
		);
}

/**
 * Subscribe to client→server chat notifications on a Connection-like peer.
 * Returns a disposer that unsubscribes every handler at once.
 */
export function handleChatNotifications(
	peer: NotifyPeer,
	handlers: ChatNotificationHandlers,
): () => void {
	const offs: Array<() => void> = [];
	if (handlers.send) offs.push(peer.on("chat/send", handlers.send as (p: unknown) => void));
	if (handlers.command)
		offs.push(peer.on("chat/command", handlers.command as (p: unknown) => void));
	return () => {
		for (const off of offs) off();
	};
}
