/**
 * GatewayBackend — `SandboxBackend` impl that delegates container
 * lifecycle to a host-side `nexal-gateway` process.
 *
 * One `GatewayClient` (single WS) is shared across every agent in the
 * Bun process. Per-session calls to `acquire` translate to
 * `gateway/spawn_agent` RPCs; the gateway side is name-idempotent so
 * spawning the same `name` twice (across nexal restarts, for example)
 * reuses the existing container.
 *
 * On `release` we tell the gateway to `kill_agent` (stop + remove the
 * container). On `detach` we just `detach_agent` — container survives.
 *
 * NOTE: gateway-side cleanup of stale `agent_id` entries when a Bun
 * process disconnects is not yet implemented in the gateway, so a
 * restart can leak an `agent_id` per session. Containers are still
 * reused correctly. Tracked as a follow-up.
 */
import type {
	AcquireOptions,
	AgentClient,
	SandboxBackend,
} from "./types.ts";
import { createLog } from "../log.ts";

const log = createLog("gateway");
import { GatewayAgentClient } from "../gateway/agent_client.ts";
import type { GatewayClient } from "../gateway/client.ts";

interface Entry {
	agentId: string;
	containerName: string;
	client: AgentClient;
}

export interface GatewayBackendOptions {
}

export class GatewayBackend implements SandboxBackend {
	readonly name = "gateway";
	private readonly entries = new Map<string, Entry>();
	private readonly inflight = new Map<string, Promise<Entry>>();

	constructor(
		private readonly gateway: GatewayClient,
		private readonly options: GatewayBackendOptions = {},
	) {}

	async acquire(sessionKey: string, opts?: AcquireOptions): Promise<AgentClient> {
		const cached = this.entries.get(sessionKey);
		if (cached) return cached.client;

		const inflight = this.inflight.get(sessionKey);
		if (inflight) return (await inflight).client;

		const promise = this.spawn(sessionKey, opts).finally(() =>
			this.inflight.delete(sessionKey),
		);
		this.inflight.set(sessionKey, promise);
		const entry = await promise;
		this.entries.set(sessionKey, entry);
		return entry.client;
	}

	async release(sessionKey: string): Promise<void> {
		const entry = this.entries.get(sessionKey);
		if (!entry) return;
		this.entries.delete(sessionKey);
		try {
			await this.gateway.invoke("gateway/kill_agent", { agent_id: entry.agentId });
		} catch (err) {
			log.error(`kill_agent ${sessionKey} failed`, err);
		}
		await entry.client.close();
	}

	async releaseAll(): Promise<void> {
		await Promise.all([...this.entries.keys()].map((k) => this.release(k)));
	}

	async detach(sessionKey: string): Promise<void> {
		const entry = this.entries.get(sessionKey);
		if (!entry) return;
		this.entries.delete(sessionKey);
		try {
			await this.gateway.invoke("gateway/detach_agent", { agent_id: entry.agentId });
		} catch (err) {
			log.error(`detach_agent ${sessionKey} failed`, err);
		}
		await entry.client.close();
	}

	private async spawn(sessionKey: string, opts?: AcquireOptions): Promise<Entry> {
		await this.gateway.hello();
		const result = await this.gateway.invoke("gateway/spawn_agent", {
			name: sessionKey,
			env: opts?.env ?? {},
			labels: { "nexal.session_key": sessionKey },
		});
		const client = new GatewayAgentClient(this.gateway, result.agent_id);
		return {
			agentId: result.agent_id,
			containerName: result.container_name,
			client,
		};
	}
}
