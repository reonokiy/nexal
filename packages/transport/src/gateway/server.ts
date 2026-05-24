/**
 * Typed request-handler registration for gateway methods.
 */
import type { RequestPeer, RpcResult } from "../connection.ts";
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

export interface GatewayRequestHandlers {
	hello?: (params: GatewayMethods["gateway/hello"]["params"]) => RpcResult<GatewayMethods, "gateway/hello"> | Promise<RpcResult<GatewayMethods, "gateway/hello">>;
	spawnAgent?: (params: SpawnAgentParams) => RpcResult<GatewayMethods, "gateway/spawn_agent"> | Promise<RpcResult<GatewayMethods, "gateway/spawn_agent">>;
	killAgent?: (params: AgentIdParams) => RpcResult<GatewayMethods, "gateway/kill_agent"> | Promise<RpcResult<GatewayMethods, "gateway/kill_agent">>;
	detachAgent?: (params: AgentIdParams) => RpcResult<GatewayMethods, "gateway/detach_agent"> | Promise<RpcResult<GatewayMethods, "gateway/detach_agent">>;
	attachAgent?: (params: AttachAgentParams) => RpcResult<GatewayMethods, "gateway/attach_agent"> | Promise<RpcResult<GatewayMethods, "gateway/attach_agent">>;
	listAgents?: (params: Record<string, never>) => RpcResult<GatewayMethods, "gateway/list_agents"> | Promise<RpcResult<GatewayMethods, "gateway/list_agents">>;
	registerProxy?: (params: RegisterProxyParams) => RpcResult<GatewayMethods, "gateway/register_proxy"> | Promise<RpcResult<GatewayMethods, "gateway/register_proxy">>;
	unregisterProxy?: (params: UnregisterProxyParams) => RpcResult<GatewayMethods, "gateway/unregister_proxy"> | Promise<RpcResult<GatewayMethods, "gateway/unregister_proxy">>;
	registerStreamProxy?: (params: RegisterStreamProxyParams) => RpcResult<GatewayMethods, "gateway/register_stream_proxy"> | Promise<RpcResult<GatewayMethods, "gateway/register_stream_proxy">>;
	unregisterStreamProxy?: (params: UnregisterStreamProxyParams) => RpcResult<GatewayMethods, "gateway/unregister_stream_proxy"> | Promise<RpcResult<GatewayMethods, "gateway/unregister_stream_proxy">>;
}

export function handleGatewayRequests(peer: RequestPeer, handlers: GatewayRequestHandlers): void {
	if (handlers.hello) peer.handleRequest("gateway/hello", handlers.hello as (params: unknown) => unknown);
	if (handlers.spawnAgent) peer.handleRequest("gateway/spawn_agent", handlers.spawnAgent as (params: unknown) => unknown);
	if (handlers.killAgent) peer.handleRequest("gateway/kill_agent", handlers.killAgent as (params: unknown) => unknown);
	if (handlers.detachAgent) peer.handleRequest("gateway/detach_agent", handlers.detachAgent as (params: unknown) => unknown);
	if (handlers.attachAgent) peer.handleRequest("gateway/attach_agent", handlers.attachAgent as (params: unknown) => unknown);
	if (handlers.listAgents) peer.handleRequest("gateway/list_agents", handlers.listAgents as (params: unknown) => unknown);
	if (handlers.registerProxy) peer.handleRequest("gateway/register_proxy", handlers.registerProxy as (params: unknown) => unknown);
	if (handlers.unregisterProxy) peer.handleRequest("gateway/unregister_proxy", handlers.unregisterProxy as (params: unknown) => unknown);
	if (handlers.registerStreamProxy) peer.handleRequest("gateway/register_stream_proxy", handlers.registerStreamProxy as (params: unknown) => unknown);
	if (handlers.unregisterStreamProxy) peer.handleRequest("gateway/unregister_stream_proxy", handlers.unregisterStreamProxy as (params: unknown) => unknown);
}
