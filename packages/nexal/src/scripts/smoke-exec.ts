/**
 * Smoke test for ExecServerClient over WebSocket, connecting directly
 * to a nexal-agent binary (no container).
 *
 * Start a nexal-agent first:
 *   target/release/nexal-agent --listen ws://127.0.0.1:4777
 *
 * Then run:
 *   NEXAL_AGENT_URL=ws://127.0.0.1:4777 bun run src/scripts/smoke-exec.ts
 */
import { ExecServerClient } from "../exec-client.ts";

const url = process.env.NEXAL_AGENT_URL ?? "ws://127.0.0.1:4777";
const client = new ExecServerClient({ url });

await client.connect();
console.log("connected");
const init = await client.initialize("smoke-exec");
console.log("initialize →", init);

const result = await client.runCommand(["/bin/bash", "-c", "echo hello from nexal-agent && pwd && date"], {
	cwd: "/tmp",
	timeoutMs: 5_000,
});
console.log("runCommand →", JSON.stringify(result, null, 2));

await client.close();
console.log("closed");
