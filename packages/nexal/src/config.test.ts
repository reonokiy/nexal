import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { loadConfig } from "./config.ts";

// Every test creates its own config file + sets NEXAL_CONFIG_PATH.
// Because loadConfig() also reads NEXAL_* env vars, tests carefully
// avoid setting anything beyond their own subset and clean up after.
const dirsToClean: string[] = [];
const envBackup: Record<string, string | undefined> = {};

async function withConfig(toml: string): Promise<string> {
	const dir = await mkdtemp(join(tmpdir(), "nexal-cfg-"));
	dirsToClean.push(dir);
	const path = join(dir, "config.toml");
	await writeFile(path, toml);
	return path;
}

function setEnv(k: string, v: string | undefined): void {
	if (!(k in envBackup)) envBackup[k] = process.env[k];
	if (v === undefined) delete process.env[k];
	else process.env[k] = v;
}

afterEach(async () => {
	for (const k of Object.keys(envBackup)) setEnv(k, envBackup[k]);
	for (const k of Object.keys(envBackup)) delete envBackup[k];
	await Promise.all(dirsToClean.map((d) => rm(d, { recursive: true, force: true })));
	dirsToClean.length = 0;
});

describe("loadConfig defaults", () => {
	test("falls back to built-in defaults with no file / no env", async () => {
		setEnv("NEXAL_CONFIG_PATH", "/nonexistent/nexal-config.toml");
		// Clear any ambient NEXAL_* env that could leak in.
		for (const k of Object.keys(process.env).filter((k) => k.startsWith("NEXAL_"))) {
			if (k !== "NEXAL_CONFIG_PATH") setEnv(k, undefined);
		}
		const cfg = await loadConfig();
		expect(cfg.debounceSecs).toBe(1);
		expect(cfg.messageDelaySecs).toBe(10);
		expect(cfg.activeWindowSecs).toBe(60);
		expect(cfg.admins).toEqual([]);
		expect(cfg.gateway.clientName).toBe("nexal-bun");
		expect(cfg.gateway.url).toBe("ws://127.0.0.1:5500");
		expect(cfg.workers.maxConcurrent).toBe(5);
		expect(cfg.executor.proxies).toEqual([]);
	});
});

describe("TOML overlay", () => {
	test("top-level scalars overwrite defaults", async () => {
		const path = await withConfig(`
debounce_secs = 7
message_delay_secs = 42
active_window_secs = 123
admins = ["alice", "bob"]
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		const cfg = await loadConfig();
		expect(cfg.debounceSecs).toBe(7);
		expect(cfg.messageDelaySecs).toBe(42);
		expect(cfg.activeWindowSecs).toBe(123);
		expect(cfg.admins).toEqual(["alice", "bob"]);
	});

	test("gateway section overlays URL + token + client_name", async () => {
		const path = await withConfig(`
[gateway]
url = "ws://example:6000"
token = "s3cret"
client_name = "ci-runner"
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		const cfg = await loadConfig();
		expect(cfg.gateway.url).toBe("ws://example:6000");
		expect(cfg.gateway.token).toBe("s3cret");
		expect(cfg.gateway.clientName).toBe("ci-runner");
	});

	test("workers section overlays url + maxConcurrent (snake_case)", async () => {
		const path = await withConfig(`
[workers]
url = "postgres://user:pw@db/nexal"
max_concurrent = 8
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		const cfg = await loadConfig();
		expect(cfg.workers.url).toBe("postgres://user:pw@db/nexal");
		expect(cfg.workers.maxConcurrent).toBe(8);
	});

	test("executor.proxies accepts snake_case upstream_url", async () => {
		const path = await withConfig(`
[[executor.proxies]]
name = "jina"
upstream_url = "https://api.jina.ai"

[executor.proxies.headers]
Authorization = "Bearer KEY"
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		const cfg = await loadConfig();
		expect(cfg.executor.proxies).toHaveLength(1);
		expect(cfg.executor.proxies[0]).toEqual({
			name: "jina",
			upstreamUrl: "https://api.jina.ai",
			headers: { Authorization: "Bearer KEY" },
		});
	});

	test("executor.proxies drops entries missing name or upstream", async () => {
		const path = await withConfig(`
[[executor.proxies]]
name = "good"
upstream_url = "https://api"

[[executor.proxies]]
# no name
upstream_url = "https://nope"

[[executor.proxies]]
name = "no-url"
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		const cfg = await loadConfig();
		expect(cfg.executor.proxies).toHaveLength(1);
		expect(cfg.executor.proxies[0]?.name).toBe("good");
	});
});

describe("env overlay", () => {
	test("NEXAL_* scalars win over TOML", async () => {
		const path = await withConfig(`debounce_secs = 1`);
		setEnv("NEXAL_CONFIG_PATH", path);
		setEnv("NEXAL_DEBOUNCE_SECS", "99");
		const cfg = await loadConfig();
		expect(cfg.debounceSecs).toBe(99);
	});

	test("NEXAL_GATEWAY__URL + __TOKEN override", async () => {
		const path = await withConfig(`
[gateway]
url = "ws://file:5500"
token = "from-file"
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		setEnv("NEXAL_GATEWAY__URL", "ws://env:9999");
		setEnv("NEXAL_GATEWAY__TOKEN", "env-token");
		const cfg = await loadConfig();
		expect(cfg.gateway.url).toBe("ws://env:9999");
		expect(cfg.gateway.token).toBe("env-token");
	});

	test("NEXAL_WORKERS__URL overrides TOML", async () => {
		const path = await withConfig(`
[workers]
url = "postgres://file"
`);
		setEnv("NEXAL_CONFIG_PATH", path);
		setEnv("NEXAL_WORKERS__URL", "postgres://env");
		const cfg = await loadConfig();
		expect(cfg.workers.url).toBe("postgres://env");
	});

	test("NEXAL_ADMINS splits CSV", async () => {
		setEnv("NEXAL_CONFIG_PATH", "/nonexistent");
		setEnv("NEXAL_ADMINS", "alice,bob, carol ");
		const cfg = await loadConfig();
		expect(cfg.admins).toEqual(["alice", "bob", "carol"]);
	});
});
