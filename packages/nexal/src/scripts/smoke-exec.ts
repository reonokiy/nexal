/**
 * Smoke test for ExecServerClient over stdio.
 *   NEXAL_EXEC_SERVER_BIN=/path/to/nexal-exec-server bun run src/scripts/smoke-exec.ts
 */
import { ExecServerClient } from "../exec-client.ts";

const binary = process.env.NEXAL_EXEC_SERVER_BIN ?? "/home/lean/i/nexal/target/release/nexal-exec-server";
const client = new ExecServerClient({ cmd: [binary, "--listen", "stdio"] });

await client.connect();
console.log("spawned");
const init = await client.initialize("smoke-exec");
console.log("initialize →", init);

const result = await client.runCommand(["/bin/bash", "-c", "echo hello from exec-server && pwd && date"], {
	cwd: "/tmp",
	timeoutMs: 5_000,
});
console.log("runCommand →", JSON.stringify(result, null, 2));

await client.close();
console.log("closed");
