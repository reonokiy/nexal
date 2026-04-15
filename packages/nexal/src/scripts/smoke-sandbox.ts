/**
 * Smoke the Podman sandbox backend against a real container.
 *
 *   NEXAL_SANDBOX_IMAGE=ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13 \
 *   NEXAL_AGENT_BIN=/path/to/nexal-exec-server \
 *   bun run src/scripts/smoke-sandbox.ts
 */
import { PodmanBackend } from "../sandbox/podman.ts";

const image = process.env.NEXAL_SANDBOX_IMAGE ?? "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13";
const bin = process.env.NEXAL_AGENT_BIN ?? "/home/lean/i/nexal/target/release/nexal-agent";
const sessionKey = process.env.NEXAL_SESSION ?? "smoke:sandbox";

const mgr = new PodmanBackend({
	image,
	agentBin: bin,
	memory: "256m",
	cpus: "0.5",
	pidsLimit: 128,
	network: false,
});

const client = await mgr.acquire(sessionKey);
await client.connect();
console.log("connected");
await client.initialize("smoke-sandbox");
console.log("initialized");
const r = await client.runCommand(["/bin/bash", "-c", "echo hello && pwd && id && cat /etc/os-release | head -3"], {
	cwd: "/workspace",
	timeoutMs: 10_000,
});
console.log("runCommand:", r);
await client.close();
await mgr.release(sessionKey);
console.log("released");
