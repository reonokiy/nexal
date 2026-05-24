/**
 * Gateway methods — `gateway/*` namespace.
 *
 * Lifecycle + introspection of containerized agents.
 * Used by `GatewayClient.invoke`.
 */

export interface HelloParams {
	access_key: string;
	client_name: string;
	ts: number;
	nonce: string;
	signature: string;
}
export interface HelloResponse {
	ok: boolean;
	gateway_version: string;
}

export interface SpawnAgentParams {
	name: string;
	image?: string;
	env?: Record<string, string>;
	labels?: Record<string, string>;
	workspace?: string;
	extra_ports?: number[];
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

export interface RegisterProxyParams {
	agent_id: string;
	name: string;
	upstream_url: string;
	headers?: Record<string, string>;
}
export interface RegisterProxyResponse {
	token: string;
	socket_path: string;
}

export interface UnregisterProxyParams {
	agent_id: string;
	name: string;
}

export interface RegisterStreamProxyParams {
	agent_id: string;
	name: string;
	container_port: number;
}
export interface RegisterStreamProxyResponse {
	listen_addr: string;
}

export interface UnregisterStreamProxyParams {
	agent_id: string;
	name: string;
}

export interface AgentInvokeParams {
	agent_id: string;
	method: string;
	params?: unknown;
}

export interface AgentNotifyParams {
	agent_id: string;
	method: string;
	params?: unknown;
}

/** Discriminated map used by `GatewayClient.invoke` for type inference. */
export interface GatewayMethods {
	"gateway/hello": { params: HelloParams; result: HelloResponse };
	"gateway/spawn_agent": { params: SpawnAgentParams; result: SpawnAgentResponse };
	"gateway/kill_agent": { params: AgentIdParams; result: OkResponse };
	"gateway/detach_agent": { params: AgentIdParams; result: OkResponse };
	"gateway/attach_agent": { params: AttachAgentParams; result: SpawnAgentResponse };
	"gateway/list_agents": { params: Record<string, never>; result: ListAgentsResponse };
	"gateway/register_proxy": { params: RegisterProxyParams; result: RegisterProxyResponse };
	"gateway/unregister_proxy": { params: UnregisterProxyParams; result: OkResponse };
	"gateway/register_stream_proxy": { params: RegisterStreamProxyParams; result: RegisterStreamProxyResponse };
	"gateway/unregister_stream_proxy": { params: UnregisterStreamProxyParams; result: OkResponse };
	"agent/invoke": { params: AgentInvokeParams; result: unknown };
}
