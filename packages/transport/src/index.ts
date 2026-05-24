/**
 * Public surface of `@nexal/transport`.
 *
 * Layout (mirrors the Rust `nexal-utils-transport` crate):
 *   - `codec.ts`, `wire.ts`        — msgpack frame + envelope
 *   - `transport.ts`               — WebSocket transport + heartbeat
 *   - `connection.ts`              — Connection / Stream + WS connect helpers
 *   - `agent/`, `gateway/`, `chat/` — per-protocol method matrix +
 *     typed client/server factories (`createAgentClient`,
 *     `createGatewayClient`, `createChatClient` / `handleAgentRequests`,
 *     `handleGatewayRequests`, `handleChatRequests`, `handleChatNotifications`)
 */

// ── Wire layer ────────────────────────────────────────────────────

export { encodeFrame, decodeFrame } from "./codec.ts";

export {
	isWireRequest,
	isWireResponse,
	isWireNotification,
} from "./wire.ts";
export type {
	WireMessage,
	WireRequest,
	WireResponse,
	WireNotification,
	WireError,
	MessageId,
} from "./wire.ts";

// ── Transport layer ───────────────────────────────────────────────

export { createWebSocketTransport, createAcceptedWebSocketTransport } from "./transport.ts";
export type {
	AcceptedWebSocketTransport,
	HeartbeatOptions,
	ReconnectOptions,
	Transport,
	TransportOptions,
	WebSocketPeer,
} from "./transport.ts";

// ── Connection layer ──────────────────────────────────────────────

export {
	Connection,
	Stream,
	WireErrorMessage,
	createAcceptedWebSocketConnection,
	createWebSocketConnection,
} from "./connection.ts";
export type {
	AcceptedWebSocketConnection,
	NotifyPeer,
	RequestHandler,
	RequestPeer,
	RpcParams,
	RpcPeer,
	RpcResult,
	WebSocketConnection,
	WebSocketConnectionOptions,
} from "./connection.ts";

// ── Protocol: agent ───────────────────────────────────────────────

export type {
	AgentMethods,
	InitializedParams,
	InitializedResponse,
	InitializeParams,
	InitializeResponse,
	StreamKind,
	ProcessStartParams,
	ProcessStartResponse,
	ProcessReadParams,
	ProcessChunk,
	ProcessReadResponse,
	ProcessTerminateParams,
	ProcessTerminateResponse,
	ProcessWriteParams,
	ProcessWriteResponse,
	FsReadFileParams,
	FsReadFileResponse,
	FsWriteFileParams,
	FsWriteFileResponse,
	FsCreateDirectoryParams,
	FsCreateDirectoryResponse,
	FsGetMetadataParams,
	FsGetMetadataResponse,
	FsReadDirectoryParams,
	FsReadDirectoryEntry,
	FsReadDirectoryResponse,
	FsRemoveParams,
	FsRemoveResponse,
	FsCopyParams,
	FsCopyResponse,
	ProxyRegisterParams,
	ProxyRegisterResponse,
	ProxyUnregisterParams,
	ProxyUnregisterResponse,
} from "./agent/methods.ts";

export type {
	AgentNotifications,
	AgentNotification,
	UnknownAgentNotification,
	ProcessOutputNotif,
	ProcessExitedNotif,
	ProcessClosedNotif,
} from "./agent/notifications.ts";

export { createAgentClient, createGatewayAgentClient } from "./agent/client.ts";
export type { AgentClient, GatewayAgentClient } from "./agent/client.ts";

export { handleAgentRequests } from "./agent/server.ts";
export type { AgentRequestHandlers } from "./agent/server.ts";

// ── Protocol: gateway ─────────────────────────────────────────────

export type {
	GatewayMethods,
	HelloParams,
	HelloResponse,
	SpawnAgentParams,
	SpawnAgentResponse,
	AgentIdParams,
	OkResponse,
	AttachAgentParams,
	AgentSummary,
	ListAgentsResponse,
	RegisterProxyParams,
	RegisterProxyResponse,
	UnregisterProxyParams,
	RegisterStreamProxyParams,
	RegisterStreamProxyResponse,
	UnregisterStreamProxyParams,
	AgentInvokeParams,
	AgentNotifyParams,
} from "./gateway/methods.ts";

export { createGatewayClient } from "./gateway/client.ts";
export type { GatewayClient } from "./gateway/client.ts";

export { handleGatewayRequests } from "./gateway/server.ts";
export type { GatewayRequestHandlers } from "./gateway/server.ts";

// ── Protocol: chat ────────────────────────────────────────────────

export type {
	ChatMethods,
	ChatNotifications,
	ChatAuthenticateParams,
	ChatAuthenticateResult,
	ChatListCommandsParams,
	ChatListCommandsResult,
	ChatSendParams,
	ChatCommandParams,
	ChatReplyParams,
	ChatTypingParams,
	ChatReplyChunkParams,
	ChatReplyEndParams,
	ChatCommandResultParams,
	CommandInfo,
	ImageBlock,
	ReplyMetadata,
} from "./chat/methods.ts";

export { createChatClient } from "./chat/client.ts";
export type { ChatClient } from "./chat/client.ts";

export { handleChatRequests, handleChatNotifications } from "./chat/server.ts";
export type { ChatRequestHandlers, ChatNotificationHandlers } from "./chat/server.ts";
