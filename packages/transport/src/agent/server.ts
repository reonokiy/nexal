/**
 * Typed request-handler registration for agent methods.
 */
import type { RequestPeer, RpcResult } from "../connection.ts";
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
} from "./methods.ts";

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
