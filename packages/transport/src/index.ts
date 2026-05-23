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
} from "./gateway.ts";

export type {
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
} from "./agent.ts";

export type {
	AgentNotifications,
	AgentNotification,
	UnknownAgentNotification,
	ProcessOutputNotif,
	ProcessExitedNotif,
	ProcessClosedNotif,
} from "./notifications.ts";

export { createWebSocketTransport } from "./transport.ts";
export type { Transport } from "./transport.ts";
