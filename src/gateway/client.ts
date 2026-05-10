/**
 * GatewayClient — single WebTransport multiplexer between the Bun
 * frontend and a `nexal-gateway` instance.
 *
 * Wire protocol: JSON-RPC 2.0, snake_case keys, fully typed via
 * `protocol.ts`'s `GatewayMethods` / `AgentMethods` /
 * `AgentNotifications` discriminated maps.
 *
 * Transport:
 *   - TCP mode: WebTransport (QUIC/HTTP3) via @webtransport-bun/webtransport
 *   - Unix socket mode: raw newline-delimited JSON over a Unix stream
 *
 * Lifecycle:
 *   1. `connect()` — open transport, wait for handshake.
 *   2. `hello()` — send `gateway/hello`. Idempotent.
 *   3. `invoke(method, params)` — typed gateway/* calls.
 *      `invokeAgent(agentId, method, params)` — typed agent/* calls,
 *      wrapped in the `agent/invoke` envelope.
 *   4. `subscribe(handler)` — receive `agent/notify` events.
 */

import type {
	AgentMethods,
	AgentNotification,
	AgentNotifications,
	GatewayMethods,
	UnknownAgentNotification,
} from "./protocol.ts";
import type { AgentClient } from "./agent_client.ts";
import { GatewayAgentClient } from "./agent_client.ts";
import { createLog } from "../log.ts";

const log = createLog("gateway-client");

interface AgentEntry {
	agentId: string;
	containerName: string;
	client: AgentClient;
}

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

export class GatewayError extends Error {
	constructor(
		message: string,
		readonly code: number,
		readonly data?: unknown,
	) {
		super(message);
		this.name = "GatewayError";
	}
}

export interface GatewayClientOptions {
	/** WebTransport URL, e.g. `"https://127.0.0.1:5500"`. */
	url: string;
	/** Unix domain socket path. When set, `url` is ignored for transport. */
	unix?: string;
	/** Shared auth token. */
	token: string;
	/** Identifier sent in `gateway/hello`. */
	clientName: string;
	connectTimeoutMs?: number;
}

export type NotificationHandler = (
	notif: AgentNotification | UnknownAgentNotification,
) => void;

const NOTIFICATION_METHODS = new Set<keyof AgentNotifications>([
	"process/output",
	"process/exited",
	"process/closed",
]);

/** Minimal interface for send/close — works for both WS wrapper and stream wrapper. */
interface Transport {
	send(data: string): void;
	close(): void;
}

export class GatewayClient {
	private transport: Transport | null = null;
	private readonly pending = new Map<JsonRpcId, Pending>();
	private readyPromise: Promise<void> | null = null;
	private helloPromise: Promise<void> | null = null;
	private readonly handlers = new Set<NotificationHandler>();

	// ── Agent session management ──────────────────────────────────────
	private readonly agents = new Map<string, AgentEntry>();
	private readonly agentInflight = new Map<string, Promise<AgentEntry>>();

	constructor(private readonly options: GatewayClientOptions) {}

	async connect(): Promise<void> {
		if (this.readyPromise) return this.readyPromise;
		this.readyPromise = this.options.unix
			? this.connectUnix()
			: this.connectWebTransport();
		return this.readyPromise;
	}

	private async connectWebTransport(): Promise<void> {
		const { WebTransport } = await import("@webtransport-bun/webtransport");
		const wt = new WebTransport(this.options.url, {
			tls: { insecureSkipVerify: true },
		} as never);
		await wt.ready;
		const stream = await wt.createBidirectionalStream();

		// Build a line-oriented reader from the readable side.
		const reader = stream.readable.getReader();
		let buffer = "";
		const readLoop = async () => {
			try {
				while (true) {
					const { value, done } = await reader.read();
					if (done) break;
					buffer += new TextDecoder().decode(value);
					const lines = buffer.split("\n");
					buffer = lines.pop()!;
					for (const line of lines) {
						if (line.trim()) this.dispatch(line);
					}
				}
			} catch {
				// stream closed
			}
			this.onDisconnect();
		};
		readLoop();

		const writer = stream.writable.getWriter();
		const encoder = new TextEncoder();
		this.transport = {
			send: (data: string) => {
				writer.write(encoder.encode(data + "\n"));
			},
			close: () => {
				writer.close();
				wt.close();
			},
		};
	}

	private async connectUnix(): Promise<void> {
		const { createConnection } = await import("node:net");
		return new Promise<void>((resolve, reject) => {
			const sock = createConnection(this.options.unix!, () => {
				resolve();
			});
			sock.on("error", (err: Error) => {
				reject(new Error(`gateway unix connect error: ${err.message}`));
			});

			let buffer = "";
			sock.on("data", (chunk: Buffer) => {
				buffer += chunk.toString("utf-8");
				const lines = buffer.split("\n");
				buffer = lines.pop()!;
				for (const line of lines) {
					if (line.trim()) this.dispatch(line);
				}
			});
			sock.on("close", () => this.onDisconnect());

			this.transport = {
				send: (data: string) => sock.write(data + "\n"),
				close: () => sock.destroy(),
			};
		});
	}

	private onDisconnect(): void {
		const closed = new Error("gateway transport closed");
		for (const p of this.pending.values()) p.reject(closed);
		this.pending.clear();
		this.transport = null;
		this.readyPromise = null;
		this.helloPromise = null;
	}

	/** Send `gateway/hello`. Idempotent — calling twice is safe. */
	async hello(): Promise<void> {
		if (this.helloPromise) return this.helloPromise;
		this.helloPromise = (async () => {
			await this.connect();
			await this.invoke("gateway/hello", {
				token: this.options.token,
				client_name: this.options.clientName,
			});
		})();
		return this.helloPromise;
	}

	/** Typed JSON-RPC call to a gateway/* method. */
	async invoke<M extends keyof GatewayMethods>(
		method: M,
		params: GatewayMethods[M]["params"],
	): Promise<GatewayMethods[M]["result"]> {
		const id = crypto.randomUUID();
		const promise = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.requireOpen().send(
			JSON.stringify({ jsonrpc: "2.0", id, method, params }),
		);
		return (await promise) as GatewayMethods[M]["result"];
	}

	/** Typed forwarded call to an agent/* method via `agent/invoke`. */
	async invokeAgent<M extends keyof AgentMethods>(
		agentId: string,
		method: M,
		params: AgentMethods[M]["params"],
	): Promise<AgentMethods[M]["result"]> {
		const id = crypto.randomUUID();
		const promise = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.requireOpen().send(
			JSON.stringify({
				jsonrpc: "2.0",
				id,
				method: "agent/invoke",
				params: { agent_id: agentId, method, params },
			}),
		);
		return (await promise) as AgentMethods[M]["result"];
	}

	/** Subscribe to `agent/notify` notifications. */
	subscribe(handler: NotificationHandler): () => void {
		this.handlers.add(handler);
		return () => this.handlers.delete(handler);
	}

	// ── Agent lifecycle ───────────────────────────────────────────────

	async acquireAgent(
		sessionKey: string,
		opts?: { env?: Record<string, string> },
	): Promise<AgentClient> {
		const cached = this.agents.get(sessionKey);
		if (cached) return cached.client;

		const inflight = this.agentInflight.get(sessionKey);
		if (inflight) return (await inflight).client;

		const promise = this.spawnAgent(sessionKey, opts).finally(() =>
			this.agentInflight.delete(sessionKey),
		);
		this.agentInflight.set(sessionKey, promise);
		const entry = await promise;
		this.agents.set(sessionKey, entry);
		return entry.client;
	}

	async releaseAgent(sessionKey: string): Promise<void> {
		const entry = this.agents.get(sessionKey);
		if (!entry) return;
		this.agents.delete(sessionKey);
		try {
			await this.invoke("gateway/kill_agent", { agent_id: entry.agentId });
		} catch (err) {
			log.error(`failed to kill agent for session "${sessionKey}"`, err);
		}
		await entry.client.close();
	}

	async detachAgent(sessionKey: string): Promise<void> {
		const entry = this.agents.get(sessionKey);
		if (!entry) return;
		this.agents.delete(sessionKey);
		try {
			await this.invoke("gateway/detach_agent", { agent_id: entry.agentId });
		} catch (err) {
			log.error(`failed to detach agent for session "${sessionKey}"`, err);
		}
		await entry.client.close();
	}

	async releaseAllAgents(): Promise<void> {
		await Promise.all([...this.agents.keys()].map((k) => this.releaseAgent(k)));
	}

	private async spawnAgent(
		sessionKey: string,
		opts?: { env?: Record<string, string> },
	): Promise<AgentEntry> {
		await this.hello();
		const result = await this.invoke("gateway/spawn_agent", {
			name: sessionKey,
			env: opts?.env ?? {},
			labels: { "nexal.session_key": sessionKey },
		});
		const client = new GatewayAgentClient(this, result.agent_id);
		return {
			agentId: result.agent_id,
			containerName: result.container_name,
			client,
		};
	}

	async close(): Promise<void> {
		this.transport?.close();
		this.transport = null;
		this.readyPromise = null;
		this.helloPromise = null;
	}

	// ── Internals ─────────────────────────────────────────────────────

	private requireOpen(): Transport {
		if (!this.transport) {
			throw new Error("gateway transport not connected — call connect() first");
		}
		return this.transport;
	}

	private dispatch(line: string): void {
		let msg: JsonRpcResponse | JsonRpcNotification;
		try {
			msg = JSON.parse(line);
		} catch {
			log.error(`received non-JSON frame from gateway, dropping: ${line.slice(0, 120)}`);
			return;
		}
		if ("id" in msg && msg.id !== undefined) {
			const slot = this.pending.get(msg.id);
			if (!slot) return;
			this.pending.delete(msg.id);
			if (msg.error) {
				slot.reject(
					new GatewayError(msg.error.message, msg.error.code, msg.error.data),
				);
			} else {
				slot.resolve(msg.result);
			}
			return;
		}
		const notif = msg as JsonRpcNotification;
		if (notif.method !== "agent/notify") return;
		const params = notif.params as
			| { agent_id?: string; method?: string; params?: unknown }
			| undefined;
		if (!params?.agent_id || !params.method) return;
		const event: AgentNotification | UnknownAgentNotification = NOTIFICATION_METHODS.has(
			params.method as keyof AgentNotifications,
		)
			? ({
					agentId: params.agent_id,
					method: params.method,
					params: params.params,
			  } as AgentNotification)
			: {
					agentId: params.agent_id,
					method: params.method,
					params: params.params,
			  };
		for (const h of this.handlers) {
			try {
				h(event);
			} catch (err) {
				log.error(`notification handler threw for ${event.method} on agent ${event.agentId}`, err);
			}
		}
	}
}
