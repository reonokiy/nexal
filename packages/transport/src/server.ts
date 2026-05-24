import type {
	AgentIdParams,
	AttachAgentParams,
	GatewayMethods,
	RegisterProxyParams,
	RegisterStreamProxyParams,
	SpawnAgentParams,
	UnregisterProxyParams,
	UnregisterStreamProxyParams,
} from "./gateway.ts";
import type {
	AgentMethods,
	FsCopyParams,
	FsCreateDirectoryParams,
	FsGetMetadataParams,
	FsReadDirectoryParams,
	FsReadFileParams,
	FsRemoveParams,
	FsWriteFileParams,
	InitializeParams,
	ProcessReadParams,
	ProcessStartParams,
	ProcessTerminateParams,
	ProcessWriteParams,
	ProxyRegisterParams,
	ProxyUnregisterParams,
} from "./agent.ts";

interface RequestPeer {
	handleRequest(method: string, handler: (params: unknown) => unknown | Promise<unknown>): void;
}

type RpcResult<M, K extends keyof M> = M[K] extends { result: infer R } ? R : never;

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

export interface AgentRequestHandlers {
	initialize?: (params: InitializeParams) => RpcResult<AgentMethods, "initialize"> | Promise<RpcResult<AgentMethods, "initialize">>;
	initialized?: () => RpcResult<AgentMethods, "initialized"> | Promise<RpcResult<AgentMethods, "initialized">>;
	processStart?: (params: ProcessStartParams) => RpcResult<AgentMethods, "process/start"> | Promise<RpcResult<AgentMethods, "process/start">>;
	processRead?: (params: ProcessReadParams) => RpcResult<AgentMethods, "process/read"> | Promise<RpcResult<AgentMethods, "process/read">>;
	processTerminate?: (params: ProcessTerminateParams) => RpcResult<AgentMethods, "process/terminate"> | Promise<RpcResult<AgentMethods, "process/terminate">>;
	processWrite?: (params: ProcessWriteParams) => RpcResult<AgentMethods, "process/write"> | Promise<RpcResult<AgentMethods, "process/write">>;
	fsReadFile?: (params: FsReadFileParams) => RpcResult<AgentMethods, "fs/read_file"> | Promise<RpcResult<AgentMethods, "fs/read_file">>;
	fsWriteFile?: (params: FsWriteFileParams) => RpcResult<AgentMethods, "fs/write_file"> | Promise<RpcResult<AgentMethods, "fs/write_file">>;
	fsCreateDirectory?: (params: FsCreateDirectoryParams) => RpcResult<AgentMethods, "fs/create_directory"> | Promise<RpcResult<AgentMethods, "fs/create_directory">>;
	fsGetMetadata?: (params: FsGetMetadataParams) => RpcResult<AgentMethods, "fs/get_metadata"> | Promise<RpcResult<AgentMethods, "fs/get_metadata">>;
	fsReadDirectory?: (params: FsReadDirectoryParams) => RpcResult<AgentMethods, "fs/read_directory"> | Promise<RpcResult<AgentMethods, "fs/read_directory">>;
	fsRemove?: (params: FsRemoveParams) => RpcResult<AgentMethods, "fs/remove"> | Promise<RpcResult<AgentMethods, "fs/remove">>;
	fsCopy?: (params: FsCopyParams) => RpcResult<AgentMethods, "fs/copy"> | Promise<RpcResult<AgentMethods, "fs/copy">>;
	proxyRegister?: (params: ProxyRegisterParams) => RpcResult<AgentMethods, "proxy/register"> | Promise<RpcResult<AgentMethods, "proxy/register">>;
	proxyUnregister?: (params: ProxyUnregisterParams) => RpcResult<AgentMethods, "proxy/unregister"> | Promise<RpcResult<AgentMethods, "proxy/unregister">>;
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

export function handleAgentRequests(peer: RequestPeer, handlers: AgentRequestHandlers): void {
	if (handlers.initialize) peer.handleRequest("initialize", handlers.initialize as (params: unknown) => unknown);
	if (handlers.initialized) peer.handleRequest("initialized", handlers.initialized as (params: unknown) => unknown);
	if (handlers.processStart) peer.handleRequest("process/start", handlers.processStart as (params: unknown) => unknown);
	if (handlers.processRead) peer.handleRequest("process/read", handlers.processRead as (params: unknown) => unknown);
	if (handlers.processTerminate) peer.handleRequest("process/terminate", handlers.processTerminate as (params: unknown) => unknown);
	if (handlers.processWrite) peer.handleRequest("process/write", handlers.processWrite as (params: unknown) => unknown);
	if (handlers.fsReadFile) peer.handleRequest("fs/read_file", handlers.fsReadFile as (params: unknown) => unknown);
	if (handlers.fsWriteFile) peer.handleRequest("fs/write_file", handlers.fsWriteFile as (params: unknown) => unknown);
	if (handlers.fsCreateDirectory) peer.handleRequest("fs/create_directory", handlers.fsCreateDirectory as (params: unknown) => unknown);
	if (handlers.fsGetMetadata) peer.handleRequest("fs/get_metadata", handlers.fsGetMetadata as (params: unknown) => unknown);
	if (handlers.fsReadDirectory) peer.handleRequest("fs/read_directory", handlers.fsReadDirectory as (params: unknown) => unknown);
	if (handlers.fsRemove) peer.handleRequest("fs/remove", handlers.fsRemove as (params: unknown) => unknown);
	if (handlers.fsCopy) peer.handleRequest("fs/copy", handlers.fsCopy as (params: unknown) => unknown);
	if (handlers.proxyRegister) peer.handleRequest("proxy/register", handlers.proxyRegister as (params: unknown) => unknown);
	if (handlers.proxyUnregister) peer.handleRequest("proxy/unregister", handlers.proxyUnregister as (params: unknown) => unknown);
}
