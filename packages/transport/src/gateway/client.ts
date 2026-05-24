/**
 * Typed client for gateway methods, layered on a `RpcPeer`.
 */
import type { RpcPeer, RpcResult } from "../connection.ts";
import type {
	AgentIdParams,
	AttachAgentParams,
	GatewayMethods,
	RegisterProxyParams,
	RegisterStreamProxyParams,
	SpawnAgentParams,
	UnregisterProxyParams,
	UnregisterStreamProxyParams,
} from "./methods.ts";

export function createGatewayClient(peer: RpcPeer) {
	return {
		hello: (params: GatewayMethods["gateway/hello"]["params"]) =>
			peer.request("gateway/hello", params) as Promise<RpcResult<GatewayMethods, "gateway/hello">>,
		spawnAgent: (params: SpawnAgentParams) =>
			peer.request("gateway/spawn_agent", params) as Promise<RpcResult<GatewayMethods, "gateway/spawn_agent">>,
		killAgent: (params: AgentIdParams) =>
			peer.request("gateway/kill_agent", params) as Promise<RpcResult<GatewayMethods, "gateway/kill_agent">>,
		detachAgent: (params: AgentIdParams) =>
			peer.request("gateway/detach_agent", params) as Promise<RpcResult<GatewayMethods, "gateway/detach_agent">>,
		attachAgent: (params: AttachAgentParams) =>
			peer.request("gateway/attach_agent", params) as Promise<RpcResult<GatewayMethods, "gateway/attach_agent">>,
		listAgents: () =>
			peer.request("gateway/list_agents", {}) as Promise<RpcResult<GatewayMethods, "gateway/list_agents">>,
		registerProxy: (params: RegisterProxyParams) =>
			peer.request("gateway/register_proxy", params) as Promise<RpcResult<GatewayMethods, "gateway/register_proxy">>,
		unregisterProxy: (params: UnregisterProxyParams) =>
			peer.request("gateway/unregister_proxy", params) as Promise<RpcResult<GatewayMethods, "gateway/unregister_proxy">>,
		registerStreamProxy: (params: RegisterStreamProxyParams) =>
			peer.request("gateway/register_stream_proxy", params) as Promise<RpcResult<GatewayMethods, "gateway/register_stream_proxy">>,
		unregisterStreamProxy: (params: UnregisterStreamProxyParams) =>
			peer.request("gateway/unregister_stream_proxy", params) as Promise<RpcResult<GatewayMethods, "gateway/unregister_stream_proxy">>,
	};
}

export type GatewayClient = ReturnType<typeof createGatewayClient>;
