/**
 * JSON-RPC 2.0 client for `nexal-exec-server` over **stdio**.
 *
 * The server is spawned as a child process with `--listen stdio`:
 * each line on stdin is a JSON-RPC message, each line on stdout is a
 * response or notification (see `crates/exec-server/src/connection.rs
 * → JsonRpcConnection::from_stdio`). stdio is the cleanest transport:
 *
 *   - No port allocation / firewall concerns
 *   - One exec-server per sandbox session, trivially isolated
 *   - The Rust-fork `tokio-tungstenite` refuses connections from stock
 *     WebSocket clients (verified with Bun and Node `ws`), so stdio is
 *     also the *only* transport that actually works here.
 *
 * This client exposes the subset needed by the bash tool:
 *
 *   - `initialize(clientName)`
 *   - `runCommand(argv, options)`  → collects stdout/stderr + exit code
 *
 * Future extension points: fs/* methods, notification stream for
 * live streaming tool output.
 */

import { randomUUID } from "node:crypto";
import type { Subprocess } from "bun";

type JsonRpcId = string | number;

interface JsonRpcResponse {
	jsonrpc: "2.0";
	id: JsonRpcId;
	result?: unknown;
	error?: { code: number; message: string; data?: unknown };
}

interface JsonRpcNotification {
	jsonrpc: "2.0";
	method: string;
	params?: unknown;
}

interface Pending {
	resolve: (v: unknown) => void;
	reject: (err: Error) => void;
}

export interface ExecServerOptions {
	/**
	 * Command that speaks the exec-server stdio JSON-RPC protocol.
	 * Typical values:
	 *   - Local: `["/path/to/nexal-exec-server", "--listen", "stdio"]`
	 *   - Containerized: `["podman", "exec", "-i", "nexal-<name>", "nexal-exec-server", "--listen", "stdio"]`
	 */
	cmd: string[];
	/** Extra env for the child process. */
	env?: Record<string, string>;
}

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

export class ExecServerClient {
	private proc: Subprocess<"pipe", "pipe", "inherit"> | null = null;
	private readonly pending = new Map<JsonRpcId, Pending>();
	private readerTask: Promise<void> | null = null;

	constructor(private readonly options: ExecServerOptions) {}

	async connect(): Promise<void> {
		if (this.proc) return;
		this.proc = Bun.spawn({
			cmd: this.options.cmd,
			stdin: "pipe",
			stdout: "pipe",
			stderr: "inherit",
			env: { ...process.env, ...(this.options.env ?? {}) } as Record<string, string>,
		}) as Subprocess<"pipe", "pipe", "inherit">;
		this.readerTask = this.readLoop();
	}

	async initialize(clientName: string): Promise<{ defaultShell?: string; cwd?: string }> {
		const resp = (await this.call("initialize", { clientName })) as {
			defaultShell?: string;
			cwd?: string;
		};
		// LSP-style handshake: server gates all other methods behind an
		// `initialized` notification from the client.
		await this.notify("initialized", {});
		return resp;
	}

	private async notify(method: string, params: unknown): Promise<void> {
		if (!this.proc) await this.connect();
		const msg = { jsonrpc: "2.0" as const, method, params };
		await this.proc!.stdin.write(JSON.stringify(msg) + "\n");
		await this.proc!.stdin.flush();
	}

	async runCommand(argv: string[], options: RunCommandOptions = {}): Promise<RunCommandResult> {
		const processId = options.processId ?? randomUUID();
		await this.call("process/start", {
			processId,
			argv,
			cwd: options.cwd ?? "/tmp",
			env: options.env ?? {},
			tty: false,
			arg0: null,
		});

		let stdout = "";
		let stderr = "";
		let exitCode = 0;
		// IMPORTANT: track the last chunk `seq` we've actually *seen*, not
		// the server's `next_seq`. The server filters with strict `>`, so
		// passing `next_seq` as `after_seq` silently drops any chunk whose
		// seq == next_seq — which happens whenever the process emits a
		// new stdout chunk between our reads, or when exit/closed bumps
		// next_seq past the last chunk. Using `last seen chunk seq`
		// guarantees we never miss a chunk.
		let afterSeq = 0;
		let exited = false;
		let timedOut = false;
		const start = Date.now();

		while (!exited) {
			if (options.timeoutMs !== undefined && Date.now() - start > options.timeoutMs) {
				timedOut = true;
				await this.call("process/terminate", { processId }).catch(() => undefined);
				break;
			}
			const resp = (await this.call("process/read", {
				processId,
				afterSeq,
				maxBytes: 1 << 20,
				waitMs: 100,
			})) as {
				chunks: Array<{ seq: number; stream: "stdout" | "stderr" | "pty"; chunk: string }>;
				nextSeq: number;
				exited: boolean;
				exitCode: number | null;
				closed: boolean;
				failure: string | null;
			};
			for (const c of resp.chunks) {
				const text = Buffer.from(c.chunk, "base64").toString("utf8");
				if (c.stream === "stderr") stderr += text;
				else stdout += text;
				if (c.seq > afterSeq) afterSeq = c.seq;
			}
			if (resp.exited) {
				exited = true;
				exitCode = resp.exitCode ?? 0;
			}
			if (resp.failure) throw new Error(`exec-server process failed: ${resp.failure}`);
		}

		return { stdout, stderr, exitCode, timedOut };
	}

	async close(): Promise<void> {
		this.proc?.kill();
		this.proc = null;
		await this.readerTask?.catch(() => undefined);
		this.readerTask = null;
		for (const p of this.pending.values()) p.reject(new Error("exec-server closed"));
		this.pending.clear();
	}

	private async call(method: string, params: unknown): Promise<unknown> {
		if (!this.proc) await this.connect();
		const id = randomUUID();
		const req = { jsonrpc: "2.0" as const, id, method, params };
		const p = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		const line = JSON.stringify(req) + "\n";
		await this.proc!.stdin.write(line);
		await this.proc!.stdin.flush();
		return p;
	}

	private async readLoop(): Promise<void> {
		const reader = this.proc!.stdout.getReader();
		const decoder = new TextDecoder();
		let buf = "";
		try {
			while (true) {
				const { value, done } = await reader.read();
				if (done) break;
				buf += decoder.decode(value, { stream: true });
				let idx: number;
				while ((idx = buf.indexOf("\n")) !== -1) {
					const line = buf.slice(0, idx).trim();
					buf = buf.slice(idx + 1);
					if (!line) continue;
					this.dispatch(line);
				}
			}
		} catch (err) {
			console.error("[exec-client] read loop error", err);
		} finally {
			for (const p of this.pending.values()) p.reject(new Error("exec-server stdout closed"));
			this.pending.clear();
		}
	}

	private dispatch(line: string): void {
		let msg: JsonRpcResponse | JsonRpcNotification;
		try {
			msg = JSON.parse(line);
		} catch {
			console.error("[exec-client] non-JSON line dropped:", line.slice(0, 120));
			return;
		}
		if ("id" in msg && msg.id !== undefined) {
			const slot = this.pending.get(msg.id);
			if (!slot) return;
			this.pending.delete(msg.id);
			if (msg.error) slot.reject(new Error(`${msg.error.code}: ${msg.error.message}`));
			else slot.resolve(msg.result);
		}
		// Notifications ignored in the polling client.
	}
}
