/**
 * GatewayAgentClient — `AgentClient` impl that forwards every call to
 * a specific `agent_id` on a shared `GatewayClient`.
 *
 * runCommand polls the agent's `process/read` for chunks. We could
 * also feed off `agent/notify` / `process/output` to avoid the poll,
 * but polling matches the existing `nexal-agent` exec semantics and
 * keeps this layer simple.
 */
import type { GatewayClient } from "./client.ts";

export interface RunCommandOptions {
	cwd?: string;
	env?: Record<string, string>;
	timeoutMs?: number;
	processId?: string;
}

export interface RunCommandResult {
	stdout: string;
	stderr: string;
	exitCode: number;
	timedOut: boolean;
}

/**
 * What the bash tool consumes. Concrete impl: `GatewayAgentClient`.
 */
export interface AgentClient {
	/**
	 * Stable id for this agent on the gateway.
	 */
	readonly agentId?: string;
	/** Run a command and accumulate output until exit. */
	runCommand(argv: string[], opts?: RunCommandOptions): Promise<RunCommandResult>;
	/** Close any per-client resources. Does NOT kill the underlying container. */
	close(): Promise<void>;
}

export class GatewayAgentClient implements AgentClient {
	constructor(
		private readonly gateway: GatewayClient,
		readonly agentId: string,
	) {}

	async runCommand(
		argv: string[],
		options: RunCommandOptions = {},
	): Promise<RunCommandResult> {
		const processId = options.processId ?? crypto.randomUUID();

		await this.gateway.invokeAgent(this.agentId, "process/start", {
			process_id: processId,
			argv,
			cwd: options.cwd ?? "/workspace",
			env: options.env ?? {},
			tty: false,
			arg0: null,
		});

		let stdout = "";
		let stderr = "";
		let exitCode = 0;
		// last-seen chunk seq, NOT next_seq from the server (see the
		// long-standing comment in the previous exec-client.ts impl).
		let afterSeq = 0;
		let exited = false;
		let timedOut = false;
		const start = Date.now();

		while (!exited) {
			if (options.timeoutMs !== undefined && Date.now() - start > options.timeoutMs) {
				timedOut = true;
				await this.gateway
					.invokeAgent(this.agentId, "process/terminate", { process_id: processId })
					.catch(() => undefined);
				break;
			}
			const resp = await this.gateway.invokeAgent(this.agentId, "process/read", {
				process_id: processId,
				after_seq: afterSeq,
				max_bytes: 1 << 20,
				wait_ms: 100,
			});
			for (const c of resp.chunks) {
				const text = Buffer.from(c.chunk, "base64").toString("utf8");
				if (c.stream === "stderr") stderr += text;
				else stdout += text;
				if (c.seq > afterSeq) afterSeq = c.seq;
			}
			if (resp.exited) {
				exited = true;
				exitCode = resp.exit_code ?? 0;
			}
			if (resp.failure) {
				throw new Error(`nexal-agent process failed: ${resp.failure}`);
			}
		}

		return { stdout, stderr, exitCode, timedOut };
	}

	/** No-op: the underlying GatewayClient WS is shared, not owned. */
	async close(): Promise<void> {}
}
