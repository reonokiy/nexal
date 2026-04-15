import { describe, expect, test } from "bun:test";

import type { GatewayClient } from "./client.ts";
import {
	type AgentMethods,
	type ProcessReadResponse,
	type ProcessStartResponse,
	type ProcessTerminateResponse,
} from "./protocol.ts";
import { GatewayAgentClient } from "./agent_client.ts";

/**
 * Minimal GatewayClient stand-in — only `invokeAgent` is exercised by
 * GatewayAgentClient. Other methods throw so we notice if the surface
 * widens and tests need updating.
 */
function stubGateway(
	handler: <M extends keyof AgentMethods>(
		agentId: string,
		method: M,
		params: AgentMethods[M]["params"],
	) => Promise<AgentMethods[M]["result"]>,
): GatewayClient {
	return {
		async invokeAgent(agentId: string, method: any, params: any) {
			return handler(agentId, method, params);
		},
		invoke: () => {
			throw new Error("invoke unused in GatewayAgentClient");
		},
		connect: () => {
			throw new Error("connect unused");
		},
		hello: () => {
			throw new Error("hello unused");
		},
		subscribe: () => {
			throw new Error("subscribe unused");
		},
		close: () => Promise.resolve(),
	} as unknown as GatewayClient;
}

function b64(s: string): string {
	return Buffer.from(s).toString("base64");
}

describe("GatewayAgentClient", () => {
	test("agentId is exposed on the client", () => {
		const c = new GatewayAgentClient(stubGateway(async () => ({} as any)), "abc-123");
		expect(c.agentId).toBe("abc-123");
	});

	test("runCommand sends process/start then polls process/read until exited", async () => {
		const calls: Array<{ method: string; params: unknown }> = [];
		let readCount = 0;
		const gw = stubGateway(async (_id, method, params) => {
			calls.push({ method, params });
			if (method === "process/start") {
				return { process_id: (params as any).process_id } as ProcessStartResponse;
			}
			if (method === "process/read") {
				readCount++;
				if (readCount === 1) {
					return {
						chunks: [{ seq: 1, stream: "stdout", chunk: b64("hello\n") }],
						next_seq: 2,
						exited: false,
						exit_code: null,
						closed: false,
						failure: null,
					} as ProcessReadResponse;
				}
				return {
					chunks: [{ seq: 2, stream: "stderr", chunk: b64("oops\n") }],
					next_seq: 3,
					exited: true,
					exit_code: 0,
					closed: true,
					failure: null,
				} as ProcessReadResponse;
			}
			throw new Error(`unexpected method ${method}`);
		});
		const client = new GatewayAgentClient(gw, "agent-1");
		const result = await client.runCommand(["/bin/echo", "hi"], {
			cwd: "/workspace",
			env: { FOO: "bar" },
		});
		expect(result.stdout).toBe("hello\n");
		expect(result.stderr).toBe("oops\n");
		expect(result.exitCode).toBe(0);
		expect(result.timedOut).toBe(false);

		const start = calls.find((c) => c.method === "process/start")!.params as any;
		expect(start.argv).toEqual(["/bin/echo", "hi"]);
		expect(start.cwd).toBe("/workspace");
		expect(start.env).toEqual({ FOO: "bar" });
		expect(start.tty).toBe(false);
		expect(start.arg0).toBeNull();

		// Second read asks after_seq = 1 (last-seen chunk seq, NOT server's next_seq)
		const reads = calls.filter((c) => c.method === "process/read").map((c) => c.params as any);
		expect(reads[0].after_seq).toBe(0);
		expect(reads[1].after_seq).toBe(1);
	});

	test("runCommand uses default cwd /workspace when not provided", async () => {
		let capturedCwd: string | undefined;
		const gw = stubGateway(async (_id, method, params) => {
			if (method === "process/start") {
				capturedCwd = (params as any).cwd;
				return { process_id: (params as any).process_id } as ProcessStartResponse;
			}
			return {
				chunks: [],
				next_seq: 0,
				exited: true,
				exit_code: 0,
				closed: true,
				failure: null,
			} as ProcessReadResponse;
		});
		await new GatewayAgentClient(gw, "agent-2").runCommand(["true"]);
		expect(capturedCwd).toBe("/workspace");
	});

	test("runCommand propagates a user-supplied processId", async () => {
		let usedPid = "";
		const gw = stubGateway(async (_id, method, params) => {
			if (method === "process/start") {
				usedPid = (params as any).process_id;
				return { process_id: usedPid } as ProcessStartResponse;
			}
			return {
				chunks: [],
				next_seq: 0,
				exited: true,
				exit_code: 0,
				closed: true,
				failure: null,
			} as ProcessReadResponse;
		});
		await new GatewayAgentClient(gw, "x").runCommand(["true"], { processId: "pid-42" });
		expect(usedPid).toBe("pid-42");
	});

	test("runCommand returns timedOut=true and calls process/terminate on timeout", async () => {
		const terminateCalls: unknown[] = [];
		const gw = stubGateway(async (_id, method, params) => {
			if (method === "process/start") {
				return { process_id: (params as any).process_id } as ProcessStartResponse;
			}
			if (method === "process/read") {
				// Never exit — always return no-chunks.
				return {
					chunks: [],
					next_seq: 0,
					exited: false,
					exit_code: null,
					closed: false,
					failure: null,
				} as ProcessReadResponse;
			}
			if (method === "process/terminate") {
				terminateCalls.push(params);
				return { running: true } as ProcessTerminateResponse;
			}
			throw new Error(`unexpected ${method}`);
		});
		const result = await new GatewayAgentClient(gw, "x").runCommand(["sleep", "999"], {
			timeoutMs: 50,
		});
		expect(result.timedOut).toBe(true);
		expect(terminateCalls).toHaveLength(1);
	});

	test("runCommand throws when agent reports process failure", async () => {
		const gw = stubGateway(async (_id, method, params) => {
			if (method === "process/start") {
				return { process_id: (params as any).process_id } as ProcessStartResponse;
			}
			return {
				chunks: [],
				next_seq: 0,
				exited: false,
				exit_code: null,
				closed: false,
				failure: "spawn: No such file",
			} as ProcessReadResponse;
		});
		await expect(
			new GatewayAgentClient(gw, "x").runCommand(["nope"], { timeoutMs: 5000 }),
		).rejects.toThrow(/spawn: No such file/);
	});

	test("runCommand captures exit_code from the final process/read response", async () => {
		const gw = stubGateway(async (_id, method, params) => {
			if (method === "process/start") {
				return { process_id: (params as any).process_id } as ProcessStartResponse;
			}
			return {
				chunks: [{ seq: 1, stream: "stdout", chunk: b64("bye") }],
				next_seq: 2,
				exited: true,
				exit_code: 42,
				closed: true,
				failure: null,
			} as ProcessReadResponse;
		});
		const r = await new GatewayAgentClient(gw, "x").runCommand(["false"]);
		expect(r.exitCode).toBe(42);
		expect(r.stdout).toBe("bye");
	});
});
