import { afterEach, describe, expect, test } from "bun:test";
import { createAcceptedWebSocketConnection, createWebSocketConnection } from "../connection.ts";

const METHODS = [
	"gateway/hello",
	"gateway/spawn_agent",
	"gateway/kill_agent",
	"gateway/detach_agent",
	"gateway/attach_agent",
	"gateway/list_agents",
	"gateway/register_proxy",
	"gateway/unregister_proxy",
	"gateway/register_stream_proxy",
	"gateway/unregister_stream_proxy",
	"agent/invoke",
	"initialize",
	"initialized",
	"process/start",
	"process/read",
	"process/write",
	"process/terminate",
	"fs/read_file",
	"fs/write_file",
	"fs/create_directory",
	"fs/get_metadata",
	"fs/read_directory",
	"fs/remove",
	"fs/copy",
	"proxy/register",
	"proxy/unregister",
] as const;

type Method = typeof METHODS[number];

const servers: Array<{ stop(): void }> = [];
const processes: Array<ReturnType<typeof Bun.spawn>> = [];

afterEach(() => {
	for (const server of servers.splice(0)) server.stop();
	for (const proc of processes.splice(0)) proc.kill();
});

describe("transport method matrix", () => {
	test("TS client -> TS server covers every concrete method", async () => {
		const url = await startTsServer();
		await runTsClient(url);
	}, 30_000);

	test("TS client -> Rust server covers every concrete method", async () => {
		const { url } = await startRustServer();
		await runTsClient(url);
	}, 30_000);

	test("Rust client -> TS server covers every concrete method", async () => {
		const url = await startTsServer();
		const out = await runRustClient(url);
		expect(out).toContain(`ok ${METHODS.length}`);
	}, 30_000);

	test("Rust client -> Rust server covers every concrete method", async () => {
		const { url } = await startRustServer();
		const out = await runRustClient(url);
		expect(out).toContain(`ok ${METHODS.length}`);
	}, 30_000);
});

async function startTsServer(): Promise<string> {
	type Data = { transport?: ReturnType<typeof createAcceptedWebSocketConnection>["transport"] };
	const server = Bun.serve<Data>({
		port: 0,
		fetch(req, server) {
			if (server.upgrade(req, { data: {} })) return;
			return new Response("upgrade failed", { status: 500 });
		},
		websocket: {
			open(ws) {
				const { transport, connection } = createAcceptedWebSocketConnection(ws);
				ws.data.transport = transport;
				for (const method of METHODS) {
					connection.handleRequest(method, () => sampleResult(method));
				}
			},
			message(ws, message) {
				ws.data.transport?.receive(message as ArrayBuffer | Uint8Array | string);
			},
			close(ws) {
				ws.data.transport?.disconnect();
			},
		},
	});
	servers.push(server);
	return `ws://${server.hostname}:${server.port}`;
}

async function runTsClient(url: string): Promise<void> {
	const { connection } = await createWebSocketConnection(url, { connectTimeoutMs: 10_000 });
	try {
		for (const method of METHODS) {
			const result = await connection.request(method, sampleParams(method));
			expect(normalize(result)).toEqual(normalize(sampleResult(method)));
		}
	} finally {
		connection.close();
	}
}

async function startRustServer(): Promise<{ url: string; proc: ReturnType<typeof Bun.spawn> }> {
	const proc = Bun.spawn(["cargo", "run", "-q", "-p", "nexal-utils-transport", "--bin", "transport-method-server"], {
		cwd: workspaceRoot(),
		stdout: "pipe",
		stderr: "pipe",
	});
	processes.push(proc);
	const url = (await readFirstLine(proc.stdout)).trim();
	if (!url.startsWith("ws://")) {
		const stderr = await new Response(proc.stderr).text();
		throw new Error(`rust server did not print ws url: ${url}\n${stderr}`);
	}
	return { url, proc };
}

async function runRustClient(url: string): Promise<string> {
	const proc = Bun.spawn(["cargo", "run", "-q", "-p", "nexal-utils-transport", "--bin", "transport-method-client", "--", url], {
		cwd: workspaceRoot(),
		stdout: "pipe",
		stderr: "pipe",
	});
	const [stdout, stderr, code] = await Promise.all([
		new Response(proc.stdout).text(),
		new Response(proc.stderr).text(),
		proc.exited,
	]);
	if (code !== 0) throw new Error(`rust client failed (${code}): ${stderr}`);
	return stdout;
}

async function readFirstLine(stream: ReadableStream<Uint8Array>): Promise<string> {
	const reader = stream.getReader();
	const decoder = new TextDecoder();
	let buf = "";
	while (true) {
		const { done, value } = await reader.read();
		if (done) return buf;
		buf += decoder.decode(value, { stream: true });
		const idx = buf.indexOf("\n");
		if (idx >= 0) return buf.slice(0, idx);
	}
}

function workspaceRoot(): string {
	return new URL("../../..", import.meta.url).pathname;
}

function normalize(value: unknown): unknown {
	if (value instanceof Uint8Array) return [...value];
	if (Array.isArray(value)) return value.map(normalize);
	if (value && typeof value === "object") {
		return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, normalize(v)]));
	}
	return value;
}

function sampleParams(method: Method): unknown {
	switch (method) {
		case "gateway/hello": return { access_key: "ak", client_name: "ts-client", ts: 1, nonce: "n", signature: "s" };
		case "gateway/spawn_agent": return { name: "agent", env: {}, labels: {}, extra_ports: [] };
		case "gateway/kill_agent":
		case "gateway/detach_agent": return { agent_id: "agent-1" };
		case "gateway/attach_agent": return { container_name: "container-1" };
		case "gateway/list_agents":
		case "initialized": return {};
		case "gateway/register_proxy": return { agent_id: "agent-1", name: "proxy", upstream_url: "https://example.com", headers: {} };
		case "gateway/unregister_proxy":
		case "gateway/unregister_stream_proxy": return { agent_id: "agent-1", name: "proxy" };
		case "gateway/register_stream_proxy": return { agent_id: "agent-1", name: "tcp", container_port: 3000 };
		case "agent/invoke": return { agent_id: "agent-1", method: "initialize", params: { client_name: "ts-client" } };
		case "initialize": return { client_name: "ts-client" };
		case "process/start": return { process_id: "p1", argv: ["true"], cwd: "/workspace", env: {}, tty: false, arg0: null };
		case "process/read": return { process_id: "p1", after_seq: 0, max_bytes: 1024, wait_ms: 0 };
		case "process/write": return { process_id: "p1", chunk: new Uint8Array([104, 105]) };
		case "process/terminate": return { process_id: "p1" };
		case "fs/read_file":
		case "fs/get_metadata":
		case "fs/read_directory": return { path: "/workspace/file.txt" };
		case "fs/write_file": return { path: "/workspace/file.txt", data: new Uint8Array([104, 105]) };
		case "fs/create_directory": return { path: "/workspace/dir", recursive: true };
		case "fs/remove": return { path: "/workspace/file.txt", recursive: true, force: true };
		case "fs/copy": return { sourcePath: "/workspace/a", destinationPath: "/workspace/b", recursive: true };
		case "proxy/register": return { socket_path: "/run/nexal/proxy/p.socket", upstream_url: "https://example.com", headers: {} };
		case "proxy/unregister": return { socket_path: "/run/nexal/proxy/p.socket" };
	}
}

function sampleResult(method: Method): unknown {
	switch (method) {
		case "gateway/hello": return { ok: true, gateway_version: "test" };
		case "gateway/spawn_agent":
		case "gateway/attach_agent": return { agent_id: "agent-1", container_name: "container-1" };
		case "gateway/kill_agent":
		case "gateway/detach_agent":
		case "gateway/unregister_proxy":
		case "gateway/unregister_stream_proxy": return { ok: true };
		case "gateway/list_agents": return { agents: [{ agent_id: "agent-1", container_name: "container-1", created_at_unix_ms: 1 }] };
		case "gateway/register_proxy": return { token: "token-1", socket_path: "/run/nexal/proxy/test.socket" };
		case "gateway/register_stream_proxy": return { listen_addr: "127.0.0.1:12345" };
		case "agent/invoke": return { ok: true };
		case "initialize": return { default_shell: "/bin/bash", cwd: "/workspace" };
		case "initialized": return null;
		case "process/start": return { process_id: "p1" };
		case "process/read": return { chunks: [], next_seq: 0, exited: true, exit_code: 0, closed: true, failure: null };
		case "process/write": return { status: "accepted" };
		case "process/terminate": return { running: false };
		case "fs/read_file": return { data: new Uint8Array([104, 105]) };
		case "fs/write_file":
		case "fs/create_directory":
		case "fs/remove":
		case "fs/copy": return {};
		case "fs/get_metadata": return { isDirectory: false, isFile: true, createdAtMs: 1, modifiedAtMs: 2 };
		case "fs/read_directory": return { entries: [{ fileName: "file.txt", isDirectory: false, isFile: true }] };
		case "proxy/register":
		case "proxy/unregister": return { ok: true };
	}
}
