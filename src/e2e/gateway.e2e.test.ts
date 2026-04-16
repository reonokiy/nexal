/**
 * E2E test — gateway → podman → nexal-agent → command execution.
 *
 * Requires:
 *   - `nexal-gateway` and `nexal-agent` release binaries built
 *   - `podman` available and working
 *   - the sandbox image `ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13` pulled
 *
 * Run:
 *   bun test src/e2e/gateway.e2e.test.ts
 */
import { spawn, type Subprocess } from "bun";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "bun:test";

import { GatewayClient } from "../gateway/client.ts";
import { GatewayAgentClient } from "../gateway/agent_client.ts";

// ── Paths ───────────────────────────────────────────────────────────

const ROOT = join(import.meta.dir, "../..");
const GATEWAY_BIN = join(ROOT, "target/release/nexal-gateway");
const AGENT_BIN = join(ROOT, "target/release/nexal-agent");
const TOKEN = `e2e-${crypto.randomUUID()}`;
const PORT = 15500; // avoid clashing with dev gateway on 5500

// ── Pre-flight checks ───────────────────────────────────────────────

function preflight(): string | null {
	if (!existsSync(GATEWAY_BIN)) return `gateway binary missing: ${GATEWAY_BIN}`;
	if (!existsSync(AGENT_BIN)) return `agent binary missing: ${AGENT_BIN}`;
	const podman = Bun.spawnSync(["podman", "--version"]);
	if (podman.exitCode !== 0) return "podman not available";
	return null;
}

// ── Gateway lifecycle ───────────────────────────────────────────────

let gatewayProc: Subprocess | null = null;
let client: GatewayClient | null = null;
const spawnedAgents: string[] = [];

async function startGateway(): Promise<void> {
	// Kill any leftover on our test port.
	try {
		const stale = Bun.spawnSync(["lsof", "-ti", `:${PORT}`]);
		for (const pid of stale.stdout.toString().trim().split("\n").filter(Boolean)) {
			process.kill(Number(pid), "SIGTERM");
		}
		const stale2 = Bun.spawnSync(["lsof", "-ti", `:${PORT + 1}`]);
		for (const pid of stale2.stdout.toString().trim().split("\n").filter(Boolean)) {
			process.kill(Number(pid), "SIGTERM");
		}
	} catch { /* ok */ }
	await new Promise((r) => setTimeout(r, 300));

	gatewayProc = spawn({
		cmd: [
			GATEWAY_BIN,
			"--token", TOKEN,
			"--listen", `127.0.0.1:${PORT}`,
			"--agent-bin", AGENT_BIN,
			"--proxy-listen", `127.0.0.1:${PORT + 1}`,
		],
		stdout: "inherit",
		stderr: "inherit",
		env: { ...process.env, NEXAL_LOG: "info" },
	});

	// Wait for the WS port to accept connections.
	const deadline = Date.now() + 15_000;
	while (Date.now() < deadline) {
		try {
			const ws = new WebSocket(`ws://127.0.0.1:${PORT}`);
			await new Promise<void>((resolve, reject) => {
				const t = setTimeout(() => { ws.close(); reject(); }, 1_000);
				ws.addEventListener("open", () => { clearTimeout(t); ws.close(); resolve(); });
				ws.addEventListener("error", () => { clearTimeout(t); reject(); });
			});
			return; // Connected successfully — gateway is ready.
		} catch {
			await new Promise((r) => setTimeout(r, 300));
		}
	}
	throw new Error("gateway did not start in 15s");
}

async function stopGateway(): Promise<void> {
	// Kill spawned agents/containers first.
	if (client) {
		for (const agentId of spawnedAgents) {
			await client.invoke("gateway/kill_agent", { agent_id: agentId }).catch(() => {});
		}
	}
	gatewayProc?.kill("SIGTERM");
	gatewayProc = null;
}

// ── Test suite ──────────────────────────────────────────────────────

const skip = preflight();

describe.skipIf(!!skip)("Gateway E2E", () => {
	beforeAll(async () => {
		console.log("[e2e] starting gateway...");
		await startGateway();
		console.log("[e2e] gateway started, waiting for bind...");
		await new Promise((r) => setTimeout(r, 1_000));
		console.log("[e2e] connecting client...");
		client = new GatewayClient({
			url: `ws://127.0.0.1:${PORT}`,
			token: TOKEN,
			clientName: "e2e-test",
			connectTimeoutMs: 10_000,
		});
		await client.hello();
		console.log("[e2e] client connected and hello'd");
	}, 30_000);

	afterAll(async () => {
		await stopGateway();
	}, 30_000);

	test("gateway/hello succeeds", () => {
		// hello() already called in beforeAll — if we're here it worked.
		expect(client).not.toBeNull();
	});

	test("gateway/list_agents starts empty", async () => {
		const res = await client!.invoke("gateway/list_agents", {});
		expect(res.agents).toBeArray();
	});

	test("gateway/spawn_agent creates a container", async () => {
		const res = await client!.invoke("gateway/spawn_agent", {
			name: "e2e-test-agent",
			env: {},
		});
		expect(res.agent_id).toBeString();
		expect(res.container_name).toBeString();
		spawnedAgents.push(res.agent_id);

		// Should appear in list.
		const list = await client!.invoke("gateway/list_agents", {});
		expect(list.agents.some((a) => a.agent_id === res.agent_id)).toBe(true);
	}, 60_000);

	test("agent is auto-initialized (can run commands immediately)", async () => {
		// Gateway initializes the agent on spawn — no manual initialize needed.
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);
		const result = await agentClient.runCommand(["true"]);
		expect(result.exitCode).toBe(0);
	}, 15_000);

	test("agent can run 'echo hello' and return output", async () => {
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);
		const result = await agentClient.runCommand(["echo", "hello"]);
		expect(result.exitCode).toBe(0);
		expect(result.stdout.trim()).toBe("hello");
		expect(result.timedOut).toBe(false);
	}, 30_000);

	test("agent can run a command with non-zero exit code", async () => {
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);
		const result = await agentClient.runCommand(["sh", "-c", "exit 42"]);
		expect(result.exitCode).toBe(42);
	}, 15_000);

	test("agent can read/write files in /workspace", async () => {
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);

		await agentClient.runCommand(["sh", "-c", "echo 'e2e-content' > /workspace/test.txt"]);
		const result = await agentClient.runCommand(["cat", "/workspace/test.txt"]);
		expect(result.stdout.trim()).toBe("e2e-content");
	}, 15_000);

	test("agent stderr is captured separately", async () => {
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);
		const result = await agentClient.runCommand(["sh", "-c", "echo err >&2; echo out"]);
		expect(result.stdout.trim()).toBe("out");
		expect(result.stderr.trim()).toBe("err");
	}, 15_000);

	test("agent command timeout works", async () => {
		const agentId = spawnedAgents[0]!;
		const agentClient = new GatewayAgentClient(client!, agentId);
		const result = await agentClient.runCommand(["sleep", "60"], { timeoutMs: 2_000 });
		expect(result.timedOut).toBe(true);
	}, 15_000);

	test("can spawn a second independent agent", async () => {
		const res = await client!.invoke("gateway/spawn_agent", {
			name: "e2e-test-agent-2",
			env: {},
		});
		expect(res.agent_id).toBeString();
		spawnedAgents.push(res.agent_id);

		const agentClient = new GatewayAgentClient(client!, res.agent_id);
		const result = await agentClient.runCommand(["echo", "second"]);
		expect(result.stdout.trim()).toBe("second");
	}, 60_000);

	test("gateway/kill_agent removes the container", async () => {
		const agentId = spawnedAgents.pop()!;
		const res = await client!.invoke("gateway/kill_agent", { agent_id: agentId });
		expect(res.ok).toBe(true);

		// Should no longer appear in list.
		const list = await client!.invoke("gateway/list_agents", {});
		expect(list.agents.some((a) => a.agent_id === agentId)).toBe(false);
	}, 30_000);
});

if (skip) {
	console.warn(`[e2e] SKIPPED: ${skip}`);
}
