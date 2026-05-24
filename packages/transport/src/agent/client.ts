/**
 * Typed clients for agent methods, layered on a `RpcPeer`
 * (`Connection` or `Stream`).
 *
 * Two flavors:
 *   - `createAgentClient(peer)`        — peer talks to the agent directly
 *   - `createGatewayAgentClient(peer, agentId)` — peer talks to the gateway,
 *     which forwards via `agent/invoke`
 */
import type { RpcPeer, RpcParams, RpcResult } from "../connection.ts";
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
import type { AgentNotifications } from "./notifications.ts";

function invokeAgent<M extends keyof AgentMethods>(
	peer: RpcPeer,
	agentId: string,
	method: M,
	params: RpcParams<AgentMethods, M>,
): Promise<RpcResult<AgentMethods, M>> {
	return peer.request("agent/invoke", {
		agent_id: agentId,
		method,
		params,
	}) as Promise<RpcResult<AgentMethods, M>>;
}

export function createAgentClient(peer: RpcPeer) {
	return {
		initialize: (params: InitializeParams) =>
			peer.request("initialize", params) as Promise<RpcResult<AgentMethods, "initialize">>,
		initialized: () =>
			peer.request("initialized", null) as Promise<RpcResult<AgentMethods, "initialized">>,
		processStart: (params: ProcessStartParams) =>
			peer.request("process/start", params) as Promise<RpcResult<AgentMethods, "process/start">>,
		processRead: (params: ProcessReadParams) =>
			peer.request("process/read", params) as Promise<RpcResult<AgentMethods, "process/read">>,
		processTerminate: (params: ProcessTerminateParams) =>
			peer.request("process/terminate", params) as Promise<RpcResult<AgentMethods, "process/terminate">>,
		processWrite: (params: ProcessWriteParams) =>
			peer.request("process/write", params) as Promise<RpcResult<AgentMethods, "process/write">>,
		fsReadFile: (params: FsReadFileParams) =>
			peer.request("fs/read_file", params) as Promise<RpcResult<AgentMethods, "fs/read_file">>,
		fsWriteFile: (params: FsWriteFileParams) =>
			peer.request("fs/write_file", params) as Promise<RpcResult<AgentMethods, "fs/write_file">>,
		fsCreateDirectory: (params: FsCreateDirectoryParams) =>
			peer.request("fs/create_directory", params) as Promise<RpcResult<AgentMethods, "fs/create_directory">>,
		fsGetMetadata: (params: FsGetMetadataParams) =>
			peer.request("fs/get_metadata", params) as Promise<RpcResult<AgentMethods, "fs/get_metadata">>,
		fsReadDirectory: (params: FsReadDirectoryParams) =>
			peer.request("fs/read_directory", params) as Promise<RpcResult<AgentMethods, "fs/read_directory">>,
		fsRemove: (params: FsRemoveParams) =>
			peer.request("fs/remove", params) as Promise<RpcResult<AgentMethods, "fs/remove">>,
		fsCopy: (params: FsCopyParams) =>
			peer.request("fs/copy", params) as Promise<RpcResult<AgentMethods, "fs/copy">>,
		proxyRegister: (params: ProxyRegisterParams) =>
			peer.request("proxy/register", params) as Promise<RpcResult<AgentMethods, "proxy/register">>,
		proxyUnregister: (params: ProxyUnregisterParams) =>
			peer.request("proxy/unregister", params) as Promise<RpcResult<AgentMethods, "proxy/unregister">>,
		onProcessOutput: (handler: (params: AgentNotifications["process/output"]) => void) =>
			peer.on("process/output", handler as (params: unknown) => void),
		onProcessExited: (handler: (params: AgentNotifications["process/exited"]) => void) =>
			peer.on("process/exited", handler as (params: unknown) => void),
		onProcessClosed: (handler: (params: AgentNotifications["process/closed"]) => void) =>
			peer.on("process/closed", handler as (params: unknown) => void),
	};
}

export function createGatewayAgentClient(peer: RpcPeer, agentId: string) {
	return {
		initialize: (params: InitializeParams) => invokeAgent(peer, agentId, "initialize", params),
		initialized: () => invokeAgent(peer, agentId, "initialized", null),
		processStart: (params: ProcessStartParams) => invokeAgent(peer, agentId, "process/start", params),
		processRead: (params: ProcessReadParams) => invokeAgent(peer, agentId, "process/read", params),
		processTerminate: (params: ProcessTerminateParams) => invokeAgent(peer, agentId, "process/terminate", params),
		processWrite: (params: ProcessWriteParams) => invokeAgent(peer, agentId, "process/write", params),
		fsReadFile: (params: FsReadFileParams) => invokeAgent(peer, agentId, "fs/read_file", params),
		fsWriteFile: (params: FsWriteFileParams) => invokeAgent(peer, agentId, "fs/write_file", params),
		fsCreateDirectory: (params: FsCreateDirectoryParams) => invokeAgent(peer, agentId, "fs/create_directory", params),
		fsGetMetadata: (params: FsGetMetadataParams) => invokeAgent(peer, agentId, "fs/get_metadata", params),
		fsReadDirectory: (params: FsReadDirectoryParams) => invokeAgent(peer, agentId, "fs/read_directory", params),
		fsRemove: (params: FsRemoveParams) => invokeAgent(peer, agentId, "fs/remove", params),
		fsCopy: (params: FsCopyParams) => invokeAgent(peer, agentId, "fs/copy", params),
		proxyRegister: (params: ProxyRegisterParams) => invokeAgent(peer, agentId, "proxy/register", params),
		proxyUnregister: (params: ProxyUnregisterParams) => invokeAgent(peer, agentId, "proxy/unregister", params),
	};
}

export type AgentClient = ReturnType<typeof createAgentClient>;
export type GatewayAgentClient = ReturnType<typeof createGatewayAgentClient>;
