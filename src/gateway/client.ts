/**
 * GatewayClient — binary-frame multiplexer between the Bun
 * frontend and a `nexal-gateway` instance.
 *
 * Wire protocol: msgpack binary frames with WireRequest / WireResponse /
 * WireNotification envelope (from `@nexal/transport`). All keys are
 * snake_case to match the Rust serde encoding.
 *
 * Transport:
 *   - TCP mode: WebSocket to the gateway (wss:// or ws://).
 *   - Unix socket mode: length-prefixed binary frames over a Unix stream.
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
	Transport,
	Connection,
} from "@nexal/transport";
import type { AgentClient } from "./agent_client.ts";
import {
	WireErrorMessage,
	createGatewayClient,
	createWebSocketConnection,
} from "@nexal/transport";
import { GatewayAgentClient } from "./agent_client.ts";
import { createLog } from "../log.ts";
import { createHmac, randomBytes } from "node:crypto";

const log = createLog("gateway-client");

interface AgentEntry {
	agentId: string;
	containerName: string;
	client: AgentClient;
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

export class GatewayClient {
	private transport: Transport | null = null;
	private connection: Connection | null = null;
	private gateway: ReturnType<typeof createGatewayClient> | null = null;
	private readyPromise: Promise<void> | null = null;
	private helloPromise: Promise<void> | null = null;
	private readonly handlers = new Set<NotificationHandler>();
	private readonly agents = new Map<string, AgentEntry>();
	private readonly agentInflight = new Map<string, Promise<AgentEntry>>();

	constructor(private readonly options: GatewayClientOptions) {}

	async connect(): Promise<void> {
		if (this.readyPromise) return this.readyPromise;
		this.readyPromise = this.connectWebSocket();
		return this.readyPromise;
	}

	private async connectWebSocket(): Promise<void> {
		const { transport, connection } = await createWebSocketConnection(this.options.url, {
			connectTimeoutMs: this.options.connectTimeoutMs,
			onDisconnect: () => this.onDisconnect(),
		});
		this.transport = transport;
		this.connection = connection;
		this.gateway = createGatewayClient(connection);
		connection.on("agent/notify", (params) => this.dispatchAgentNotification(params));
	}

	private onDisconnect(): void {
		this.connection?.close();
		this.transport = null;
		this.connection = null;
		this.gateway = null;
		this.readyPromise = null;
		this.helloPromise = null;
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
			await this.requireGateway().hello({
				access_key: this.options.accessKey,
				client_name: this.options.clientName,
				ts,
				nonce,
				signature,
			});
		})();
		return this.helloPromise;
	}

	/** Typed call to a gateway/* method. */
	async invoke<M extends keyof GatewayMethods>(
		method: M,
		params: GatewayMethods[M]["params"],
	): Promise<GatewayMethods[M]["result"]> {
		try {
			return (await this.requireConnection().request(method, params)) as GatewayMethods[M]["result"];
		} catch (err) {
			throw this.mapError(err);
		}
	}

	listAgents() {
		return this.requireGateway().listAgents();
	}

	spawnAgent(params: GatewayMethods["gateway/spawn_agent"]["params"]) {
		return this.requireGateway().spawnAgent(params);
	}

	killAgent(params: GatewayMethods["gateway/kill_agent"]["params"]) {
		return this.requireGateway().killAgent(params);
	}

	detachAgentById(params: GatewayMethods["gateway/detach_agent"]["params"]) {
		return this.requireGateway().detachAgent(params);
	}

	attachAgent(params: GatewayMethods["gateway/attach_agent"]["params"]) {
		return this.requireGateway().attachAgent(params);
	}

	registerProxy(params: GatewayMethods["gateway/register_proxy"]["params"]) {
		return this.requireGateway().registerProxy(params);
	}

	unregisterProxy(params: GatewayMethods["gateway/unregister_proxy"]["params"]) {
		return this.requireGateway().unregisterProxy(params);
	}

	registerStreamProxy(params: GatewayMethods["gateway/register_stream_proxy"]["params"]) {
		return this.requireGateway().registerStreamProxy(params);
	}

	unregisterStreamProxy(params: GatewayMethods["gateway/unregister_stream_proxy"]["params"]) {
		return this.requireGateway().unregisterStreamProxy(params);
	}

	request(method: string, params?: unknown): Promise<unknown> {
		try {
			return this.requireConnection().request(method, params);
		} catch (err) {
			throw this.mapError(err);
		}
	}

	notify(method: string, params?: unknown): void {
		this.requireConnection().notify(method, params);
	}

	on(method: string, handler: (params: unknown) => void): () => void {
		return this.requireConnection().on(method, handler);
	}

	/** Typed forwarded call to an agent/* method via `agent/invoke`. */
	async invokeAgent<M extends keyof AgentMethods>(
		agentId: string,
		method: M,
		params: AgentMethods[M]["params"],
	): Promise<AgentMethods[M]["result"]> {
		try {
			return (await this.requireConnection().request("agent/invoke", {
				agent_id: agentId,
				method,
				params,
			})) as AgentMethods[M]["result"];
		} catch (err) {
			throw this.mapError(err);
		}
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

		const promise = this.spawnSessionAgent(sessionKey, opts).finally(() =>
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
			await this.requireGateway().killAgent({ agent_id: entry.agentId });
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
			await this.requireGateway().detachAgent({ agent_id: entry.agentId });
		} catch (err) {
			log.error(`failed to detach agent for session "${sessionKey}"`, err);
		}
		await entry.client.close();
	}

	async releaseAllAgents(): Promise<void> {
		await Promise.allSettled([...this.agents.keys()].map((k) => this.releaseAgent(k)));
	}

	private async spawnSessionAgent(
		sessionKey: string,
		opts?: { env?: Record<string, string> },
	): Promise<AgentEntry> {
		await this.hello();
		const result = await this.requireGateway().spawnAgent({
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
		this.connection?.close();
		this.transport?.close();
		this.transport = null;
		this.connection = null;
		this.gateway = null;
		this.readyPromise = null;
		this.helloPromise = null;
	}

	// ── Internals ─────────────────────────────────────────────────────

	private requireConnection(): Connection {
		if (!this.connection) {
			throw new Error("gateway connection not ready — call connect() first");
		}
		return this.connection;
	}

	private requireGateway(): ReturnType<typeof createGatewayClient> {
		if (!this.gateway) {
			throw new Error("gateway client not ready — call connect() first");
		}
		return this.gateway;
	}

	private dispatchAgentNotification(raw: unknown): void {
		const params = raw as { agent_id?: string; method?: string; params?: unknown } | undefined;
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

	private mapError(err: unknown): Error {
		if (err instanceof WireErrorMessage) {
			return new GatewayError(err.error.message, err.error.code, err.error.data);
		}
		return err instanceof Error ? err : new Error(String(err));
	}
}
