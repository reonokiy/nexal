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
