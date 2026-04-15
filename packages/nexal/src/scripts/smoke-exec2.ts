import { ExecServerClient } from "../exec-client.ts";
console.log("before new");
const client = new ExecServerClient({
	cmd: ["/home/lean/i/nexal/target/release/nexal-exec-server", "--listen", "stdio"],
});
console.log("before connect");
await client.connect();
console.log("connected");
const init = await client.initialize("test");
console.log("init:", init);
await client.close();
console.log("closed");
