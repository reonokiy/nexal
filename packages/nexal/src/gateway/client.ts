/**
 * GatewayClient — single WebSocket multiplexer between the Bun
 * frontend and a `nexal-gateway` instance.
 *
 * Wire protocol: JSON-RPC 2.0, snake_case keys, fully typed via
 * `protocol.ts`'s `GatewayMethods` / `AgentMethods` /
 * `AgentNotifications` discriminated maps.
 *
 * Lifecycle:
 *   1. `connect()` — open WS, wait for handshake.
 *   2. `hello()` — send `gateway/hello`. Idempotent.
 *   3. `invoke(method, params)` — typed gateway/* calls.
 *      `invokeAgent(agentId, method, params)` — typed agent/* calls,
 *      wrapped in the `agent/invoke` envelope.
 *   4. `subscribe(handler)` — receive `agent/notify` events as a
 *      typed discriminated union (or `unknown` for unmodeled methods).
 *
 * One GatewayClient is shared across ALL agents in the process. The
 * gateway side multiplexes by `agent_id`.
 */

import type {
	AgentMethods,
	AgentNotification,
	AgentNotifications,
	GatewayMethods,
	UnknownAgentNotification,
} from "./protocol.ts";

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
	/** WebSocket URL, e.g. `"ws://127.0.0.1:5500"`. */
	url: string;
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

export class GatewayClient {
	private ws: WebSocket | null = null;
	private readonly pending = new Map<JsonRpcId, Pending>();
	private readyPromise: Promise<void> | null = null;
	private helloPromise: Promise<void> | null = null;
	private readonly handlers = new Set<NotificationHandler>();

	constructor(private readonly options: GatewayClientOptions) {}

	async connect(): Promise<void> {
		if (this.readyPromise) return this.readyPromise;
		this.readyPromise = new Promise<void>((resolve, reject) => {
			const ws = new WebSocket(this.options.url);
			this.ws = ws;
			const timer = setTimeout(() => {
				reject(new Error(`gateway WS connect timed out: ${this.options.url}`));
				ws.close();
			}, this.options.connectTimeoutMs ?? 5_000);
			ws.addEventListener("open", () => {
				clearTimeout(timer);
				resolve();
			});
			ws.addEventListener("error", (ev: any) => {
				clearTimeout(timer);
				reject(new Error(`gateway WS error: ${ev?.message ?? this.options.url}`));
			});
			ws.addEventListener("close", () => {
				const closed = new Error("gateway WS closed");
				for (const p of this.pending.values()) p.reject(closed);
				this.pending.clear();
				this.ws = null;
				this.readyPromise = null;
				this.helloPromise = null;
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

	async close(): Promise<void> {
		this.ws?.close();
		this.ws = null;
		this.readyPromise = null;
		this.helloPromise = null;
	}

	// ── Internals ─────────────────────────────────────────────────────

	private requireOpen(): WebSocket {
		if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
			throw new Error("gateway WS not connected — call connect() first");
		}
		return this.ws;
	}

	private dispatch(line: string): void {
		let msg: JsonRpcResponse | JsonRpcNotification;
		try {
			msg = JSON.parse(line);
		} catch {
			console.error("[gateway-client] non-JSON frame dropped:", line.slice(0, 120));
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
		// Notification — only `agent/notify` is interesting today.
		const notif = msg as JsonRpcNotification;
		if (notif.method !== "agent/notify") return;
		const params = notif.params as
			| { agent_id?: string; method?: string; params?: unknown }
			| undefined;
		if (!params?.agent_id || !params.method) return;
		const event: AgentNotification | UnknownAgentNotification = NOTIFICATION_METHODS.has(
			params.method as keyof AgentNotifications,
		)
			? // Trust the gateway's wire format — narrowing happens in handlers
			  // via `notif.method ===` checks.
			  ({
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
				console.error("[gateway-client] notification handler threw", err);
			}
		}
	}
}
