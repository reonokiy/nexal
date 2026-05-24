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
} from "./gateway.ts";

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
} from "./agent.ts";

export type {
	AgentNotifications,
	AgentNotification,
	UnknownAgentNotification,
	ProcessOutputNotif,
	ProcessExitedNotif,
	ProcessClosedNotif,
} from "./notifications.ts";

export { Connection, Stream, WireErrorMessage } from "./connection.ts";
export type { RequestHandler } from "./connection.ts";

export { createAgentClient, createGatewayAgentClient, createGatewayClient } from "./client.ts";

export { handleAgentRequests, handleGatewayRequests } from "./server.ts";
export type { AgentRequestHandlers, GatewayRequestHandlers } from "./server.ts";

export { createAcceptedWebSocketConnection, createWebSocketConnection } from "./connect.ts";
export type {
	AcceptedWebSocketConnection,
	WebSocketConnection,
	WebSocketConnectionOptions,
} from "./connect.ts";

export { createWebSocketTransport } from "./transport.ts";
export { createAcceptedWebSocketTransport } from "./transport.ts";
export type {
	AcceptedWebSocketTransport,
	HeartbeatOptions,
	ReconnectOptions,
	Transport,
	TransportOptions,
	WebSocketPeer,
} from "./transport.ts";
