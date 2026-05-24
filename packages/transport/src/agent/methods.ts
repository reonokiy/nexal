/**
 * Agent methods — methods exposed by the in-container `nexal-agent`.
 *
 * Frontend invokes them by wrapping in `agent/invoke`.
 * Used by `GatewayClient.invokeAgent`.
 */

export interface InitializeParams {
	client_name: string;
}
export interface InitializeResponse {
	default_shell?: string;
	cwd?: string;
}

export type InitializedParams = Record<string, never> | null | undefined;
export type InitializedResponse = null;

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
	chunk: Uint8Array;
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
	chunk: Uint8Array;
}
export interface ProcessWriteResponse {
	status: "accepted" | "unknown_process" | "stdin_closed" | "starting";
}

export interface FsReadFileParams {
	path: string;
}
export interface FsReadFileResponse {
	data: Uint8Array;
}

export interface FsWriteFileParams {
	path: string;
	data: Uint8Array;
}
export type FsWriteFileResponse = Record<string, never>;

export interface FsCreateDirectoryParams {
	path: string;
	recursive?: boolean;
}
export type FsCreateDirectoryResponse = Record<string, never>;

export interface FsGetMetadataParams {
	path: string;
}
export interface FsGetMetadataResponse {
	isDirectory: boolean;
	isFile: boolean;
	createdAtMs: number;
	modifiedAtMs: number;
}

export interface FsReadDirectoryParams {
	path: string;
}
export interface FsReadDirectoryEntry {
	fileName: string;
	isDirectory: boolean;
	isFile: boolean;
}
export interface FsReadDirectoryResponse {
	entries: FsReadDirectoryEntry[];
}

export interface FsRemoveParams {
	path: string;
	recursive?: boolean;
	force?: boolean;
}
export type FsRemoveResponse = Record<string, never>;

export interface FsCopyParams {
	sourcePath: string;
	destinationPath: string;
	recursive?: boolean;
}
export type FsCopyResponse = Record<string, never>;

export interface ProxyRegisterParams {
	socket_path: string;
	upstream_url: string;
	headers?: Record<string, string>;
}
export interface ProxyRegisterResponse {
	ok: boolean;
}

export interface ProxyUnregisterParams {
	socket_path: string;
}
export interface ProxyUnregisterResponse {
	ok: boolean;
}

/** Discriminated map used by `GatewayClient.invokeAgent` for type inference. */
export interface AgentMethods {
	initialize: { params: InitializeParams; result: InitializeResponse };
	initialized: { params: InitializedParams; result: InitializedResponse };
	"process/start": { params: ProcessStartParams; result: ProcessStartResponse };
	"process/read": { params: ProcessReadParams; result: ProcessReadResponse };
	"process/terminate": { params: ProcessTerminateParams; result: ProcessTerminateResponse };
	"process/write": { params: ProcessWriteParams; result: ProcessWriteResponse };
	"fs/read_file": { params: FsReadFileParams; result: FsReadFileResponse };
	"fs/write_file": { params: FsWriteFileParams; result: FsWriteFileResponse };
	"fs/create_directory": { params: FsCreateDirectoryParams; result: FsCreateDirectoryResponse };
	"fs/get_metadata": { params: FsGetMetadataParams; result: FsGetMetadataResponse };
	"fs/read_directory": { params: FsReadDirectoryParams; result: FsReadDirectoryResponse };
	"fs/remove": { params: FsRemoveParams; result: FsRemoveResponse };
	"fs/copy": { params: FsCopyParams; result: FsCopyResponse };
	"proxy/register": { params: ProxyRegisterParams; result: ProxyRegisterResponse };
	"proxy/unregister": { params: ProxyUnregisterParams; result: ProxyUnregisterResponse };
}
