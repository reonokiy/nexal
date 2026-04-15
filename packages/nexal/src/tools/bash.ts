/**
 * bash tool — executes shell commands through the Rust `nexal-agent`
 * WebSocket. This is the only shell surface the agent sees.
 *
 * Shape matches `AgentTool` from `@mariozechner/pi-agent-core`. The
 * schema is defined with TypeBox (re-exported from pi-ai).
 */
import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static, Type } from "@mariozechner/pi-ai";

import type { ExecServerClient } from "../exec-client.ts";

export const BashParams = Type.Object({
	command: Type.String({ description: "Shell command to run inside the sandbox (passed to bash -c)." }),
	cwd: Type.Optional(Type.String({ description: "Working directory. Defaults to /workspace." })),
	timeoutMs: Type.Optional(Type.Number({ description: "Max runtime in ms. Defaults to 30s." })),
});

export interface BashDetails {
	command: string;
	cwd: string;
	exitCode: number;
	timedOut: boolean;
}

export function createBashTool(client: ExecServerClient): AgentTool<typeof BashParams, BashDetails> {
	return {
		name: "bash",
		label: "Bash",
		description: "Run a shell command inside the sandbox container. Returns stdout + stderr + exit code.",
		parameters: BashParams,
		async execute(
			_toolCallId: string,
			params: Static<typeof BashParams>,
			signal?: AbortSignal,
		): Promise<AgentToolResult<BashDetails>> {
			signal?.throwIfAborted();
			const cwd = params.cwd ?? "/workspace";
			const timeoutMs = params.timeoutMs ?? 30_000;

			const { stdout, stderr, exitCode, timedOut } = await client.runCommand(
				["/bin/bash", "-c", params.command],
				{ cwd, timeoutMs },
			);

			const parts: string[] = [];
			if (stdout) parts.push(stdout.trimEnd());
			if (stderr) parts.push(`[stderr]\n${stderr.trimEnd()}`);
			if (timedOut) parts.push(`[process killed: timeout after ${timeoutMs}ms]`);
			parts.push(`[exit=${exitCode}]`);

			return {
				content: [{ type: "text", text: parts.join("\n\n") }],
				details: { command: params.command, cwd, exitCode, timedOut },
			};
		},
	};
}
