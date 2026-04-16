import { describe, expect, mock, test } from "bun:test";

import type {
	AgentClient,
	RunCommandOptions,
	RunCommandResult,
} from "../gateway/agent_client.ts";
import { createBashTool } from "./bash.ts";

function stubClient(
	handler: (argv: string[], opts?: RunCommandOptions) => RunCommandResult,
): AgentClient {
	return {
		agentId: "stub",
		async runCommand(argv, opts) {
			return handler(argv, opts);
		},
		async close() {},
	};
}

describe("createBashTool", () => {
	test("has the expected name/label + accepts empty args via defaults", () => {
		const tool = createBashTool(stubClient(() => ({ stdout: "", stderr: "", exitCode: 0, timedOut: false })));
		expect(tool.name).toBe("bash");
		expect(tool.label).toBe("Bash");
		expect(typeof tool.description).toBe("string");
	});

	test("wraps the command in /bin/bash -c and plumbs cwd + timeout", async () => {
		const spy = mock((_argv: string[], _opts?: RunCommandOptions) => ({
			stdout: "",
			stderr: "",
			exitCode: 0,
			timedOut: false,
		}));
		const tool = createBashTool(stubClient(spy as any));
		await tool.execute("call-1", {
			command: "echo hi && ls",
			cwd: "/scratch",
			timeoutMs: 10_000,
		} as any);
		const [argv, opts] = (spy as any).mock.calls[0];
		expect(argv).toEqual(["/bin/bash", "-c", "echo hi && ls"]);
		expect(opts).toEqual({ cwd: "/scratch", timeoutMs: 10_000 });
	});

	test("defaults cwd to /workspace and timeout to 30s", async () => {
		let capturedOpts: RunCommandOptions | undefined;
		const tool = createBashTool(
			stubClient((_argv, opts) => {
				capturedOpts = opts;
				return { stdout: "", stderr: "", exitCode: 0, timedOut: false };
			}),
		);
		await tool.execute("c", { command: "whoami" } as any);
		expect(capturedOpts?.cwd).toBe("/workspace");
		expect(capturedOpts?.timeoutMs).toBe(30_000);
	});

	test("formats stdout + stderr + exit into a single text block", async () => {
		const tool = createBashTool(
			stubClient(() => ({
				stdout: "hello\n",
				stderr: "oops\n",
				exitCode: 42,
				timedOut: false,
			})),
		);
		const r = await tool.execute("c", { command: "run" } as any);
		const text = (r.content[0] as { text: string }).text;
		expect(text).toContain("hello");
		expect(text).toContain("[stderr]");
		expect(text).toContain("oops");
		expect(text).toContain("[exit=42]");
		expect(r.details.exitCode).toBe(42);
		expect(r.details.timedOut).toBe(false);
		expect(r.details.command).toBe("run");
	});

	test("includes [process killed: timeout after Nms] when the client reports timeout", async () => {
		const tool = createBashTool(
			stubClient(() => ({
				stdout: "",
				stderr: "",
				exitCode: 0,
				timedOut: true,
			})),
		);
		const r = await tool.execute("c", { command: "sleep 999", timeoutMs: 5_000 } as any);
		const text = (r.content[0] as { text: string }).text;
		expect(text).toContain("[process killed: timeout after 5000ms]");
		expect(r.details.timedOut).toBe(true);
	});

	test("honors an aborted signal by throwing", async () => {
		const tool = createBashTool(
			stubClient(() => ({ stdout: "", stderr: "", exitCode: 0, timedOut: false })),
		);
		const ac = new AbortController();
		ac.abort();
		await expect(
			tool.execute("c", { command: "noop" } as any, ac.signal),
		).rejects.toThrow();
	});

	test("empty stdout/stderr is omitted from the text", async () => {
		const tool = createBashTool(
			stubClient(() => ({ stdout: "", stderr: "", exitCode: 0, timedOut: false })),
		);
		const r = await tool.execute("c", { command: "true" } as any);
		const text = (r.content[0] as { text: string }).text;
		expect(text).not.toContain("[stderr]");
		expect(text.trim()).toBe("[exit=0]");
	});
});
