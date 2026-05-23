export { encodeFrame, decodeFrame } from "./codec.ts";
export {
	isWireRequest,
	isWireResponse,
	isWireNotification,
} from "./protocol.ts";
export type {
	// Wire envelope
	WireMessage,
	WireRequest,
	WireResponse,
	WireNotification,
	WireError,
	MessageId,
	// Gateway methods
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
	// Agent methods
	AgentMethods,
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
	// Notifications
	AgentNotifications,
	AgentNotification,
	UnknownAgentNotification,
	ProcessOutputNotif,
	ProcessExitedNotif,
	ProcessClosedNotif,
} from "./protocol.ts";
export { createWebSocketTransport, createUnixTransport } from "./transport.ts";
export type { Transport } from "./transport.ts";
