/**
 * Nexal entry — load config, start channels, wire them into the
 * AgentPool with a per-session sandboxed bash tool.
 *
 * Sandboxing is **mandatory**. Only the *backend* is configurable;
 * today only `"podman"` is implemented (see `src/sandbox/`).
 *
 * Env:
 *   NEXAL_EXEC_SERVER_BIN   (default ../../../target/release/nexal-exec-server)
 *   NEXAL_HTTP_PORT         (default 3000)
 *   NEXAL_MODEL_PROVIDER    (default "openrouter")
 *   NEXAL_MODEL             (default "openai/gpt-4o")
 *   NEXAL_SYSTEM_PROMPT     (default "You are Nexal.")
 *   NEXAL_SANDBOX_BACKEND   (default "podman")
 *   OPENROUTER_API_KEY etc. — per provider (see @mariozechner/pi-ai env-api-keys)
 */
import { getModel } from "@mariozechner/pi-ai";

import { AgentPool } from "./agent-pool.ts";
import type { Channel } from "./channels/types.ts";
import { HttpChannel } from "./channels/http.ts";
import { TelegramChannel } from "./channels/telegram.ts";
import { HeartbeatChannel } from "./channels/heartbeat.ts";
import { CronChannel } from "./channels/cron.ts";
import { loadConfig } from "./config.ts";
import { createSandboxBackend } from "./sandbox/index.ts";
import { createBashTool } from "./tools/bash.ts";

async function main(): Promise<void> {
	const cfg = await loadConfig();
	const execBin =
		process.env.NEXAL_EXEC_SERVER_BIN ??
		`${import.meta.dir}/../../../target/release/nexal-exec-server`;
	const httpPort = Number(
		(cfg.channel.http?.port as number | string | undefined) ??
			process.env.NEXAL_HTTP_PORT ??
			"3000",
	);
	const provider = process.env.NEXAL_MODEL_PROVIDER ?? "openrouter";
	const modelId = process.env.NEXAL_MODEL ?? "openai/gpt-4o";
	const systemPrompt = process.env.NEXAL_SYSTEM_PROMPT ?? "You are Nexal.";

	// Sandbox is always on. Pick a backend; default = podman (only impl today).
	const sandboxBucket = (cfg.channel.sandbox ?? {}) as Record<string, unknown>;
	const sandbox = createSandboxBackend({
		backend:
			process.env.NEXAL_SANDBOX_BACKEND ??
			(sandboxBucket.backend as string | undefined),
		config: sandboxBucket,
		defaults: { execServerBin: execBin, workspace: cfg.workspace },
	});
	console.log(`[nexal] sandbox backend: ${sandbox.name} (one sandbox per session)`);

	const model = getModel(provider as any, modelId);

	const channels = new Map<string, Channel>();
	channels.set("http", new HttpChannel({ port: httpPort }));

	const tgBucket = cfg.channel.telegram ?? {};
	const tgToken =
		(tgBucket.botToken as string | undefined) ??
		process.env.TELEGRAM_BOT_TOKEN ??
		process.env.NEXAL_TELEGRAM_BOT_TOKEN;
	if (tgToken) {
		channels.set(
			"telegram",
			new TelegramChannel({
				botToken: tgToken,
				allowFrom:
					(tgBucket.allowFrom as string[] | undefined) ??
					splitCsv(process.env.NEXAL_TELEGRAM_ALLOW_FROM),
				allowChats:
					(tgBucket.allowChats as string[] | undefined) ??
					splitCsv(process.env.NEXAL_TELEGRAM_ALLOW_CHATS),
			}),
		);
	}

	const hbCfg = cfg.channel.heartbeat ?? {};
	if (hbCfg.enabled !== false) {
		channels.set(
			"heartbeat",
			new HeartbeatChannel({
				intervalMinutes:
					(hbCfg.intervalMins as number | undefined) ??
					(hbCfg.intervalMinutes as number | undefined),
			}),
		);
	}

	const cronCfg = cfg.channel.cron ?? {};
	if (cronCfg.enabled !== false) {
		channels.set(
			"cron",
			new CronChannel({
				tickIntervalSecs: cronCfg.tickIntervalSecs as number | undefined,
			}),
		);
	}

	const pool = new AgentPool({
		systemPrompt,
		model,
		tools: [],
		toolsFor: async (key) => {
			const client = await sandbox.acquire(key);
			await client.connect();
			await client.initialize(`nexal:${key}`);
			return {
				tools: [createBashTool(client)],
				dispose: async () => {
					await client.close();
					await sandbox.release(key);
				},
			};
		},
		channels,
		debounce: {
			debounceMs: cfg.debounceSecs * 1_000,
			delayMs: cfg.messageDelaySecs * 1_000,
			activeWindowMs: cfg.activeWindowSecs * 1_000,
		},
	});

	const stop = new AbortController();
	const shutdown = async (sig: string) => {
		console.error(`[nexal] ${sig} received, shutting down`);
		stop.abort();
		await pool.shutdown();
		await Promise.all([...channels.values()].map((c) => c.stop().catch(() => undefined)));
		await sandbox.releaseAll();
		process.exit(0);
	};
	process.on("SIGINT", () => void shutdown("SIGINT"));
	process.on("SIGTERM", () => void shutdown("SIGTERM"));

	await Promise.all(
		[...channels.values()].map((channel) =>
			channel.start((msg) => {
				try {
					pool.handle(msg);
				} catch (err) {
					console.error(`[nexal] channel=${channel.name} dispatch error`, err);
				}
			}),
		),
	);

	await new Promise<void>((resolve) => {
		stop.signal.addEventListener("abort", () => resolve());
	});
}

function splitCsv(v: string | undefined): string[] | undefined {
	if (!v) return undefined;
	const parts = v.split(",").map((s) => s.trim()).filter(Boolean);
	return parts.length > 0 ? parts : undefined;
}

main().catch((err) => {
	console.error("[nexal] fatal", err);
	process.exit(1);
});
