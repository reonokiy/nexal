/**
 * Smoke the Bun → nexal-gateway wire end-to-end. No LLM, no
 * AgentRegistry — just the GatewayClient + GatewayBackend +
 * GatewayAgentClient calling shell commands via runCommand.
 *
 * Requirements:
 *   - nexal-gateway running and reachable
 *   - the gateway's [defaults].agent_bin pointing at a built nexal-agent
 *
 * Env:
 *   NEXAL_GATEWAY_URL    (default ws://127.0.0.1:5500)
 *   NEXAL_GATEWAY_TOKEN  (REQUIRED)
 */
import { GatewayClient } from "../gateway/client.ts";
import { GatewayBackend } from "../sandbox/gateway.ts";

const URL = process.env.NEXAL_GATEWAY_URL ?? "ws://127.0.0.1:5500";
const TOKEN = process.env.NEXAL_GATEWAY_TOKEN;

if (!TOKEN) throw new Error("NEXAL_GATEWAY_TOKEN env var is required");

const gateway = new GatewayClient({ url: URL, token: TOKEN, clientName: "smoke" });
await gateway.hello();
console.log("[smoke] hello ok");

const list1 = await gateway.invoke("gateway/list_agents", {});
console.log(`[smoke] list before spawn: ${list1.agents.length} agents`);

const sandbox = new GatewayBackend(gateway);
const sessionKey = "worker:smoke-gateway-bun";

const client = await sandbox.acquire(sessionKey);
console.log(`[smoke] acquired client for ${sessionKey}`);

const r = await client.runCommand(
	["/bin/bash", "-c", "whoami; pwd; echo HOME=$HOME; echo NEXAL_DATA_DIR=$NEXAL_DATA_DIR"],
	{ cwd: "/workspace", timeoutMs: 10_000 },
);
console.log("[smoke] runCommand →", r);

if (r.exitCode !== 0) throw new Error(`unexpected exit ${r.exitCode}`);
if (!r.stdout.includes("HOME=")) {
	throw new Error(`unexpected env (no HOME): ${r.stdout}`);
}

const list2 = await gateway.invoke("gateway/list_agents", {});
console.log(`[smoke] list after spawn: ${list2.agents.length} agents`);

await sandbox.release(sessionKey);
console.log("[smoke] release ok");

const list3 = await gateway.invoke("gateway/list_agents", {});
console.log(`[smoke] list after release: ${list3.agents.length} agents`);

await gateway.close();
console.log("[smoke] OK");
