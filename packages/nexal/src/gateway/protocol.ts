/**
 * Typed wire protocol mirror for `nexal-gateway` (Rust, see
 * `crates/nexal-gateway/src/protocol.rs`) and the in-container
 * `nexal-agent` (Rust, see `crates/nexal-agent/src/protocol.rs`).
 *
 * All keys are snake_case to match the Rust serde encoding. Two
 * method namespaces:
 *
 *   - `GatewayMethods`        — gateway/*: lifecycle + introspection
 *                               of containerized agents.
 *   - `AgentMethods`          — methods exposed by the in-container
 *                               nexal-agent. Frontend invokes them
 *                               by wrapping in `agent/invoke`.
 *   - `AgentNotifications`    — async events pushed from the agent
 *                               (process/output, process/exited,
 *                               process/closed). Delivered to the
 *                               frontend via `agent/notify`.
 *
 * Adding a new method:
 *   1. Add the params + response interfaces here.
 *   2. Add an entry to the relevant map (Methods / Notifications).
 *   3. The generic `invoke` / `invokeAgent` will infer everything.
 */

// ─── gateway/* params + responses ───────────────────────────────────

export interface HelloParams {
	token: string;
	client_name: string;
}
export interface HelloResponse {
	ok: boolean;
	gateway_version: string;
}

export interface SpawnAgentParams {
	/** Human-friendly suffix; gateway sanitizes + prefixes for the container name. */
	name: string;
	image?: string;
	env?: Record<string, string>;
	labels?: Record<string, string>;
	workspace?: string;
}
export interface SpawnAgentResponse {
	agent_id: string;
	container_name: string;
}

export interface AgentIdParams {
	agent_id: string;
}
export interface OkResponse {
	ok: boolean;
}

export interface AttachAgentParams {
	container_name: string;
}

export interface AgentSummary {
	agent_id: string;
	container_name: string;
	created_at_unix_ms: number;
}
export interface ListAgentsResponse {
	agents: AgentSummary[];
}

/** Discriminated map used by `GatewayClient.invoke` for type inference. */
export interface GatewayMethods {
	"gateway/hello": { params: HelloParams; result: HelloResponse };
	"gateway/spawn_agent": { params: SpawnAgentParams; result: SpawnAgentResponse };
	"gateway/kill_agent": { params: AgentIdParams; result: OkResponse };
	"gateway/detach_agent": { params: AgentIdParams; result: OkResponse };
	"gateway/attach_agent": { params: AttachAgentParams; result: SpawnAgentResponse };
	"gateway/list_agents": { params: Record<string, never>; result: ListAgentsResponse };
}

// ─── agent/* params + responses ─────────────────────────────────────

export interface InitializeParams {
	client_name: string;
}
export interface InitializeResponse {
	default_shell?: string;
	cwd?: string;
}

export type StreamKind = "stdout" | "stderr" | "pty";

export interface ProcessStartParams {
	process_id: string;
	argv: string[];
	cwd: string;
	env: Record<string, string>;
	tty: boolean;
	arg0: string | null;
	output_bytes_cap?: number;
}
export interface ProcessStartResponse {
	process_id: string;
}

export interface ProcessReadParams {
	process_id: string;
	after_seq: number;
	max_bytes: number;
	wait_ms: number;
}
export interface ProcessChunk {
	seq: number;
	stream: StreamKind;
	/** base64-encoded raw bytes. */
	chunk: string;
}
export interface ProcessReadResponse {
	chunks: ProcessChunk[];
	next_seq: number;
	exited: boolean;
	exit_code: number | null;
	closed: boolean;
	failure: string | null;
}

export interface ProcessTerminateParams {
	process_id: string;
}
export interface ProcessTerminateResponse {
	running: boolean;
}

export interface ProcessWriteParams {
	process_id: string;
	/** base64-encoded raw bytes. */
	chunk: string;
}
export interface ProcessWriteResponse {
	accepted: boolean;
}

/** Discriminated map used by `GatewayClient.invokeAgent` for type inference. */
export interface AgentMethods {
	initialize: { params: InitializeParams; result: InitializeResponse };
	"process/start": { params: ProcessStartParams; result: ProcessStartResponse };
	"process/read": { params: ProcessReadParams; result: ProcessReadResponse };
	"process/terminate": { params: ProcessTerminateParams; result: ProcessTerminateResponse };
	"process/write": { params: ProcessWriteParams; result: ProcessWriteResponse };
}

// ─── notifications (agent → gateway → frontend) ──────────────────────

export interface ProcessOutputNotif {
	process_id: string;
	stream: StreamKind;
	chunk: string;
	seq: number;
}
export interface ProcessExitedNotif {
	process_id: string;
	exit_code: number | null;
}
export interface ProcessClosedNotif {
	process_id: string;
}

/** Map of agent notification method name → params shape. */
export interface AgentNotifications {
	"process/output": ProcessOutputNotif;
	"process/exited": ProcessExitedNotif;
	"process/closed": ProcessClosedNotif;
}

/** Discriminated union — narrow by `notif.method`. */
export type AgentNotification = {
	[K in keyof AgentNotifications]: {
		agentId: string;
		method: K;
		params: AgentNotifications[K];
	};
}[keyof AgentNotifications];

/** Catch-all for notification methods we don't model yet. */
export interface UnknownAgentNotification {
	agentId: string;
	method: string;
	params: unknown;
}
