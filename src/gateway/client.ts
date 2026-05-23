/**
 * GatewayClient — WebSocket-based multiplexer between the Bun
 * frontend and a `nexal-gateway` instance.
 *
 * Wire protocol: JSON-RPC 2.0, snake_case keys, fully typed via
 * `protocol.ts`'s `GatewayMethods` / `AgentMethods` /
 * `AgentNotifications` discriminated maps.
 *
 * Transport:
 *   - TCP mode: WebSocket to the gateway (wss:// or ws://).
 *   - Unix socket mode: raw newline-delimited JSON over a Unix stream.
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
import { createHmac, randomBytes } from "node:crypto";

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
	/** WebSocket URL, e.g. `"wss://nexal.fly.dev"`. */
	url: string;
	/** Unix domain socket path. When set, `url` is ignored for transport. */
	unix?: string;
	/** Credential id (sent in `gateway/hello`). */
	accessKey: string;
	/** Secret used to HMAC-sign the handshake; never sent on the wire. */
	secretKey: string;
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
	private readonly decoder = new TextDecoder();

	private readonly agents = new Map<string, AgentEntry>();
	private readonly agentInflight = new Map<string, Promise<AgentEntry>>();

	constructor(private readonly options: GatewayClientOptions) {}

	async connect(): Promise<void> {
		if (this.readyPromise) return this.readyPromise;
		this.readyPromise = this.options.unix
			? this.connectUnix()
			: this.connectWebSocket();
		return this.readyPromise;
	}

	private async connectWebSocket(): Promise<void> {
		const url = this.options.url.replace(/^http/, "ws");
		return new Promise<void>((resolve, reject) => {
			const timeout = setTimeout(() => {
				reject(new Error(`gateway WebSocket connect timeout to ${url}`));
			}, this.options.connectTimeoutMs ?? 10_000);

			const ws = new WebSocket(url);
			ws.binaryType = "arraybuffer";

			ws.onopen = () => {
				clearTimeout(timeout);
				this.transport = {
					send: (data: string) => {
						if (ws.readyState === 1) ws.send(data);
					},
					close: () => ws.close(),
				};
				resolve();
			};

			ws.onerror = () => {
				clearTimeout(timeout);
				reject(new Error(`gateway WebSocket error connecting to ${url}`));
			};

			ws.onmessage = (ev: MessageEvent) => {
				const text =
					typeof ev.data === "string"
						? ev.data
						: this.decoder.decode(ev.data as ArrayBuffer);
				for (const line of text.split("\n")) {
					if (line.trim()) this.dispatch(line);
				}
			};

			ws.onclose = () => this.onDisconnect();
		});
	}

	private async connectUnix(): Promise<void> {
		const { createConnection } = await import("node:net");
		return new Promise<void>((resolve, reject) => {
			const sock = createConnection(this.options.unix!, () => {
				this.transport = {
					send: (data: string) => sock.write(data + "\n"),
					close: () => sock.destroy(),
				};
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
		});
	}

	private onDisconnect(): void {
		this.rejectAllPending(new Error("gateway transport closed"));
		this.transport = null;
		this.readyPromise = null;
		this.helloPromise = null;
	}

	private rejectAllPending(reason: Error): void {
		for (const p of this.pending.values()) p.reject(reason);
		this.pending.clear();
	}

	/** Send `gateway/hello`. Idempotent — calling twice is safe. */
	async hello(): Promise<void> {
		if (this.helloPromise) return this.helloPromise;
		this.helloPromise = (async () => {
			await this.connect();
			const ts = Math.floor(Date.now() / 1000);
			const nonce = randomBytes(16).toString("hex");
			const canonical = `${this.options.accessKey}\n${ts}\n${nonce}\n${this.options.clientName}`;
			const signature = createHmac("sha256", this.options.secretKey)
				.update(canonical)
				.digest("hex");
			await this.invoke("gateway/hello", {
				access_key: this.options.accessKey,
				client_name: this.options.clientName,
				ts,
				nonce,
				signature,
			});
		})();
		return this.helloPromise;
	}

	private sendRequest(method: string, params: unknown): Promise<unknown> {
		const id = crypto.randomUUID();
		const promise = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.requireOpen().send(
			JSON.stringify({ jsonrpc: "2.0", id, method, params }),
		);
		return promise;
	}

	/** Typed JSON-RPC call to a gateway/* method. */
	async invoke<M extends keyof GatewayMethods>(
		method: M,
		params: GatewayMethods[M]["params"],
	): Promise<GatewayMethods[M]["result"]> {
		return this.sendRequest(method, params) as Promise<GatewayMethods[M]["result"]>;
	}

	/** Typed forwarded call to an agent/* method via `agent/invoke`. */
	async invokeAgent<M extends keyof AgentMethods>(
		agentId: string,
		method: M,
		params: AgentMethods[M]["params"],
	): Promise<AgentMethods[M]["result"]> {
		return this.sendRequest("agent/invoke", {
			agent_id: agentId,
			method,
			params,
		}) as Promise<AgentMethods[M]["result"]>;
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
		await Promise.allSettled([...this.agents.keys()].map((k) => this.releaseAgent(k)));
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
		this.rejectAllPending(new Error("gateway client closed"));
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
		const base = {
			agentId: params.agent_id,
			method: params.method,
			params: params.params,
		};
		const event = NOTIFICATION_METHODS.has(params.method as keyof AgentNotifications)
			? (base as AgentNotification)
			: (base as UnknownAgentNotification);
		for (const h of this.handlers) {
			try {
				h(event);
			} catch (err) {
				log.error(`notification handler threw for ${event.method} on agent ${event.agentId}`, err);
			}
		}
	}
}
