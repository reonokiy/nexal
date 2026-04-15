/**
 * JSON-RPC 2.0 client for `nexal-exec-server` over **WebSocket**.
 *
 * The server is reachable at `ws://HOST:PORT` (see
 * `crates/exec-server/src/server/transport.rs`). Each WebSocket text
 * frame carries one JSON-RPC message.
 *
 * This client exposes the subset needed by the bash tool:
 *
 *   - `initialize(clientName)`   — followed by the LSP-style
 *                                  `initialized` notification
 *   - `runCommand(argv, opts)`   — collects stdout/stderr + exit code
 *
 * Future extension points: fs/* methods, switch from polling
 * `process/read` to the `process/output` notification stream.
 */

import { randomUUID } from "node:crypto";

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
	/** WebSocket URL, e.g. `"ws://127.0.0.1:4777"`. */
	url: string;
	/** How long to wait for the WS handshake before failing. */
	connectTimeoutMs?: number;
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
	private ws: WebSocket | null = null;
	private readonly pending = new Map<JsonRpcId, Pending>();
	private readyPromise: Promise<void> | null = null;

	constructor(private readonly options: ExecServerOptions) {}

	async connect(): Promise<void> {
		if (this.readyPromise) return this.readyPromise;
		this.readyPromise = new Promise<void>((resolve, reject) => {
			const ws = new WebSocket(this.options.url);
			this.ws = ws;
			const timer = setTimeout(() => {
				reject(new Error(`exec-server WS connect timed out: ${this.options.url}`));
				ws.close();
			}, this.options.connectTimeoutMs ?? 5_000);

			ws.addEventListener("open", () => {
				clearTimeout(timer);
				resolve();
			});
			ws.addEventListener("error", (ev: any) => {
				clearTimeout(timer);
				reject(new Error(`exec-server WS error: ${ev?.message ?? this.options.url}`));
			});
			ws.addEventListener("close", () => {
				for (const p of this.pending.values()) p.reject(new Error("exec-server WS closed"));
				this.pending.clear();
				this.ws = null;
				this.readyPromise = null;
			});
			ws.addEventListener("message", (ev) => {
				const data = ev.data;
				const text =
					typeof data === "string"
						? data
						: new TextDecoder().decode(new Uint8Array(data as ArrayBuffer));
				this.dispatch(text);
			});
		});
		return this.readyPromise;
	}

	async initialize(clientName: string): Promise<{ defaultShell?: string; cwd?: string }> {
		const resp = (await this.call("initialize", { clientName })) as {
			defaultShell?: string;
			cwd?: string;
		};
		// LSP-style handshake: server gates all other methods behind an
		// `initialized` notification from the client.
		this.notify("initialized", {});
		return resp;
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
		// seq == next_seq — happens when exit/closed bumps next_seq past
		// the last chunk. Using last-seen guarantees we never miss one.
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
		this.ws?.close();
		this.ws = null;
		this.readyPromise = null;
	}

	// ── Internals ─────────────────────────────────────────────────────

	private notify(method: string, params: unknown): void {
		this.requireOpen().send(JSON.stringify({ jsonrpc: "2.0", method, params }));
	}

	private call(method: string, params: unknown): Promise<unknown> {
		const id = randomUUID();
		const req = { jsonrpc: "2.0" as const, id, method, params };
		const p = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.requireOpen().send(JSON.stringify(req));
		return p;
	}

	private requireOpen(): WebSocket {
		if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
			throw new Error("exec-server WS not connected — call connect() first");
		}
		return this.ws;
	}

	private dispatch(line: string): void {
		let msg: JsonRpcResponse | JsonRpcNotification;
		try {
			msg = JSON.parse(line);
		} catch {
			console.error("[exec-client] non-JSON frame dropped:", line.slice(0, 120));
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
