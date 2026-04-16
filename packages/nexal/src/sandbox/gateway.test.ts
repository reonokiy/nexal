import { describe, expect, mock, test } from "bun:test";

import type { GatewayClient } from "../gateway/client.ts";
import { GatewayBackend } from "./gateway.ts";

/**
 * Thin GatewayClient stub — records each invoke + whose hello() is
 * awaited before the first spawn. All agent-side calls happen through
 * GatewayAgentClient.invokeAgent, which we don't exercise here.
 */
function stubGateway(opts?: {
	onHello?: () => Promise<void>;
	onSpawn?: (params: any) => Promise<{ agent_id: string; container_name: string }>;
	onKill?: (params: any) => Promise<void>;
	onDetach?: (params: any) => Promise<void>;
}): GatewayClient & { calls: Array<{ method: string; params: unknown }> } {
	const calls: Array<{ method: string; params: unknown }> = [];
	return {
		calls,
		hello: opts?.onHello ?? (async () => undefined),
		async invoke(method: string, params: any) {
			calls.push({ method, params });
			if (method === "gateway/spawn_agent") {
				return (
					opts?.onSpawn?.(params) ??
					({
						agent_id: `aid-${params.name}`,
						container_name: `nexal-worker-${params.name}`,
					} as any)
				);
			}
			if (method === "gateway/kill_agent") {
				await opts?.onKill?.(params);
				return { ok: true } as any;
			}
			if (method === "gateway/detach_agent") {
				await opts?.onDetach?.(params);
				return { ok: true } as any;
			}
			throw new Error(`unexpected invoke ${method}`);
		},
		invokeAgent: async () => ({}) as any,
		connect: async () => undefined,
		subscribe: () => () => undefined,
		close: async () => undefined,
	} as unknown as GatewayClient & { calls: Array<{ method: string; params: unknown }> };
}

describe("GatewayBackend", () => {
	test("acquire calls hello once and spawn_agent per sessionKey", async () => {
		const helloSpy = mock(async () => undefined);
		const gw = stubGateway({ onHello: helloSpy });
		const backend = new GatewayBackend(gw);
		await backend.acquire("worker:a");
		await backend.acquire("worker:b");
		expect(helloSpy).toHaveBeenCalledTimes(2);
		const spawns = gw.calls.filter((c) => c.method === "gateway/spawn_agent");
		expect(spawns.map((s) => (s.params as any).name)).toEqual(["worker:a", "worker:b"]);
	});

	test("spawn passes env + workspace + session label", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw, { defaultWorkspace: "/host/default" });
		await backend.acquire("worker:x", {
			env: { K: "v" },
			workspace: "/host/override",
		});
		const spawn = gw.calls.find((c) => c.method === "gateway/spawn_agent")!.params as any;
		expect(spawn.env).toEqual({ K: "v" });
		expect(spawn.workspace).toBe("/host/override");
		expect(spawn.labels["nexal.session_key"]).toBe("worker:x");
	});

	test("acquire returns same client for the same key (cache hit)", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw);
		const a = await backend.acquire("worker:dup");
		const b = await backend.acquire("worker:dup");
		expect(a).toBe(b);
		const spawns = gw.calls.filter((c) => c.method === "gateway/spawn_agent");
		expect(spawns).toHaveLength(1);
	});

	test("concurrent acquires for the same key dedup via inflight promise", async () => {
		let resolveSpawn!: (v: any) => void;
		const spawnPromise = new Promise<any>((r) => {
			resolveSpawn = r;
		});
		const gw = stubGateway({
			onSpawn: async (params) => {
				await spawnPromise;
				return { agent_id: `aid-${params.name}`, container_name: "ctr" };
			},
		});
		const backend = new GatewayBackend(gw);
		const p1 = backend.acquire("worker:race");
		const p2 = backend.acquire("worker:race");
		resolveSpawn(undefined);
		const [c1, c2] = await Promise.all([p1, p2]);
		expect(c1).toBe(c2);
		const spawns = gw.calls.filter((c) => c.method === "gateway/spawn_agent");
		expect(spawns).toHaveLength(1);
	});

	test("acquire falls back to defaultWorkspace when opts.workspace omitted", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw, { defaultWorkspace: "/host/workspace" });
		await backend.acquire("worker:ws");
		const spawn = gw.calls.find((c) => c.method === "gateway/spawn_agent")!.params as any;
		expect(spawn.workspace).toBe("/host/workspace");
	});

	test("release calls kill_agent with stored agent_id and drops the entry", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw);
		await backend.acquire("worker:to-kill");
		await backend.release("worker:to-kill");
		const kill = gw.calls.find((c) => c.method === "gateway/kill_agent")!.params as any;
		expect(kill.agent_id).toBe("aid-worker:to-kill");
		// Re-acquiring should spawn again.
		await backend.acquire("worker:to-kill");
		const spawns = gw.calls.filter((c) => c.method === "gateway/spawn_agent");
		expect(spawns).toHaveLength(2);
	});

	test("release on unknown sessionKey is a no-op", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw);
		await backend.release("worker:nope");
		expect(gw.calls).toEqual([]);
	});

	test("detach calls detach_agent (not kill) and drops the entry", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw);
		await backend.acquire("worker:to-detach");
		await backend.detach("worker:to-detach");
		expect(gw.calls.some((c) => c.method === "gateway/detach_agent")).toBe(true);
		expect(gw.calls.some((c) => c.method === "gateway/kill_agent")).toBe(false);
	});

	test("releaseAll kills every cached entry", async () => {
		const gw = stubGateway();
		const backend = new GatewayBackend(gw);
		await backend.acquire("worker:1");
		await backend.acquire("worker:2");
		await backend.acquire("worker:3");
		await backend.releaseAll();
		const kills = gw.calls.filter((c) => c.method === "gateway/kill_agent");
		expect(kills.map((k) => (k.params as any).agent_id).sort()).toEqual([
			"aid-worker:1",
			"aid-worker:2",
			"aid-worker:3",
		]);
	});

	test("backend.name is 'gateway'", () => {
		const backend = new GatewayBackend(stubGateway());
		expect(backend.name).toBe("gateway");
	});

	test("release tolerates a failing kill_agent", async () => {
		const gw = stubGateway({
			onKill: async () => {
				throw new Error("upstream borked");
			},
		});
		const backend = new GatewayBackend(gw);
		await backend.acquire("worker:err");
		// Shouldn't throw even though kill_agent rejects.
		await backend.release("worker:err");
		// Entry is dropped so re-acquire spawns anew.
		await backend.acquire("worker:err");
		const spawns = gw.calls.filter((c) => c.method === "gateway/spawn_agent");
		expect(spawns).toHaveLength(2);
	});
});
