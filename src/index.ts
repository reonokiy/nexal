/**
 * nexal entry — load config, connect to nexal-gateway, start channels,
 * wire them into the AgentPool. The per-chat main agent is a
 * **dispatcher only**:
 *
 *   - it has NO bash and NO sandbox of its own
 *   - its only tools are the dispatcher set: spawn_executor,
 *     spawn_shot_task, spawn_coordinator, route_to_agent, list_agents,
 *     get_agent, cancel_agent (see `tools/worker.ts`)
 *   - all real work happens inside spawned executors, each of which
 *     lives in a Podman container managed by `nexal-gateway`, and
 *     gets bash + send_update + report_to_parent
 *
 * Env:
 *   NEXAL_HTTP_PORT                 (default 3000)
 *   NEXAL_MODEL_PROVIDER            (default "openrouter")
 *   NEXAL_MODEL                     (default "openai/gpt-4o")
 *   NEXAL_COORDINATOR_SYSTEM_PROMPT (override the default coordinator prompt)
 *   NEXAL_EXECUTOR_SYSTEM_PROMPT    (override the default executor prompt)
 *   NEXAL_GATEWAY_URL               (override [gateway].url)
 *   NEXAL_GATEWAY_TOKEN             (override [gateway].token; required if not in config)
 *   OPENROUTER_API_KEY etc. — per provider (see @mariozechner/pi-ai env-api-keys)
 */
import { spawn, type Subprocess } from "bun";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import type { AgentTool } from "@mariozechner/pi-agent-core";
import { log } from "./log.ts";
import { getModel } from "@mariozechner/pi-ai";

import { AgentPool } from "./agent-pool.ts";
import type { Channel } from "./channels/types.ts";
import { HttpChannel } from "./channels/http.ts";
import { WsChannel } from "./channels/ws.ts";
import { TelegramChannel } from "./channels/telegram.ts";
import { HeartbeatChannel } from "./channels/heartbeat.ts";
import { CronChannel } from "./channels/cron.ts";
import { loadConfig } from "./config.ts";
import { GatewayClient } from "./gateway/client.ts";
import { createSandboxBackend } from "./sandbox/index.ts";
import { createBashTool } from "./tools/bash.ts";
import { createReportToParentTool } from "./tools/report_to_parent.ts";
import { createSendUpdateTool } from "./tools/send_update.ts";
import { createDispatcherTools } from "./tools/worker.ts";
import { WorkerRegistry } from "./workers/registry.ts";
import { loadAuth, loadModelConfig, closeSettings } from "./settings.ts";
import { createWorkerStore } from "./workers/store.ts";

const DEFAULT_COORDINATOR_PROMPT = [
	"You are a nexal coordinator. You DO NOT execute tasks yourself — you have no shell, no filesystem, no network.",
	"You schedule work onto agents below you and route messages between them.",
	"For every incoming message, decide:",
	"  1. Does an existing agent (use list_agents) already own this domain? If yes, route_to_agent(id, message).",
	"  2. Is this an ongoing project / role / area? Spawn an executor (spawn_executor) with a clear system_prompt that defines its identity.",
	"  3. Is this a one-shot job (single command, single fetch, single build)? Use spawn_shot_task.",
	"  4. Is the domain large enough to deserve its own scheduling layer? Spawn a sub-coordinator (spawn_coordinator) and route work to it.",
	"Executors reply to the user directly via send_update — you don't need to summarize their output. Keep your own replies short: announce routing decisions, ask for clarification when ambiguous, but never try to do the work yourself.",
].join("\n");

const DEFAULT_EXECUTOR_PROMPT = [
	"You are a nexal executor agent. You have bash inside a Podman sandbox at /workspace and one tool to talk to the user: send_update.",
	"Filesystem layout:",
	"  - /workspace — user-facing project area (empty by default).",
	"  - /run/nexal/proxy/<name>.socket — pre-registered upstream API proxies as Unix sockets. The gateway injects auth headers for you, so you NEVER see or need API keys. Use the socket directly, e.g. `curl --unix-socket /run/nexal/proxy/jina.socket http://x/v1/search?q=foo` (the host part of the URL is ignored).",
	"Do the work assigned to you. Use bash freely. Call send_update for milestones, when you need clarification, and to deliver final results.",
	"Do NOT echo every intermediate thought — each send_update call becomes a separate Telegram message.",
].join("\n");

// ── Saved auth bootstrap ────────────────────────────────────────────

async function applySavedAuth(): Promise<void> {
	try {
		// Restore model config if not overridden by env.
		const saved = await loadModelConfig();
		if (saved) {
			if (!process.env.NEXAL_MODEL_PROVIDER) process.env.NEXAL_MODEL_PROVIDER = saved.provider;
			if (!process.env.NEXAL_MODEL) process.env.NEXAL_MODEL = saved.modelId;
		}

		// Restore OAuth/API key credentials.
		const providerName = process.env.NEXAL_MODEL_PROVIDER ?? saved?.provider;
		if (!providerName) return;

		const auth = await loadAuth(providerName);
		if (!auth) return;

		if (auth.type === "oauth" && auth.access) {
			const envKey = oauthEnvKey(providerName);
			if (envKey && !process.env[envKey]) {
				// Check if token is expired and refresh if needed.
				if (auth.expires && Date.now() >= auth.expires && auth.refresh) {
					const { getOAuthApiKey } = await import("@mariozechner/pi-ai/oauth");
					const result = await getOAuthApiKey(providerName, {
						[providerName]: { refresh: auth.refresh, access: auth.access, expires: auth.expires },
					});
					if (result) {
						process.env[envKey] = result.apiKey;
						// Persist refreshed credentials.
						const { saveAuth } = await import("./settings.ts");
						await saveAuth({
							...auth,
							access: result.newCredentials.access,
							refresh: result.newCredentials.refresh,
							expires: result.newCredentials.expires,
						});
						log.info(`refreshed ${providerName} OAuth token`);
					}
				} else {
					process.env[envKey] = auth.access;
				}
				log.info(`loaded saved ${providerName} OAuth credentials`);
			}
		} else if (auth.type === "apikey" && auth.apiKey) {
			const envKey = apiKeyEnvKey(providerName);
			if (envKey && !process.env[envKey]) {
				process.env[envKey] = auth.apiKey;
				log.info(`loaded saved ${providerName} API key`);
			}
		}
	} catch (err) {
		log.error("failed to load saved auth (continuing):", err);
	}
}

function oauthEnvKey(provider: string): string | null {
	switch (provider) {
		case "anthropic": return "ANTHROPIC_OAUTH_TOKEN";
		default: return null;
	}
}

function apiKeyEnvKey(provider: string): string | null {
	switch (provider) {
		case "anthropic": return "ANTHROPIC_API_KEY";
		case "openai": return "OPENAI_API_KEY";
		case "openrouter": return "OPENROUTER_API_KEY";
		case "google": return "GOOGLE_API_KEY";
		case "mistral": return "MISTRAL_API_KEY";
		default: return null;
	}
}

// ── Embedded gateway for local dev ──────────────────────────────────

async function launchGateway(): Promise<{
	url: string;
	token: string;
	proc: Subprocess;
}> {
	const token = crypto.randomUUID();
	const url = "ws://127.0.0.1:15500";
	const projectRoot = join(import.meta.dir, "..");
	const gatewayBin = join(projectRoot, "target/release/nexal-gateway");
	const agentBin = join(projectRoot, "target/release/nexal-agent");

	if (!existsSync(gatewayBin)) {
		throw new Error(
			`nexal-gateway binary not found at ${gatewayBin} — run 'cargo build --release -p nexal-gateway' first`,
		);
	}

	log.info("no gateway token configured — auto-starting embedded gateway");

	// Kill any stale gateway from a previous run (e.g. bun --watch restart).
	try {
		const stale = Bun.spawnSync(["lsof", "-ti", ":15500"]);
		for (const pid of stale.stdout.toString().trim().split("\n").filter(Boolean)) {
			process.kill(Number(pid), "SIGTERM");
		}
	} catch { /* ok */ }

	const proc = spawn({
		cmd: [
			gatewayBin,
			"--token", token,
			"--listen", "127.0.0.1:15500",
			"--proxy-listen", "127.0.0.1:15501",
			...(existsSync(agentBin) ? ["--agent-bin", agentBin] : []),
		],
		stdout: "inherit",
		stderr: "inherit",
		env: {
			...process.env,
			NEXAL_LOG: process.env.NEXAL_LOG ?? "info",
		},
	});

	// Poll until the TCP port accepts WS connections.
	const deadline = Date.now() + 10_000;
	while (Date.now() < deadline) {
		try {
			await new Promise<void>((resolve, reject) => {
				const ws = new WebSocket(url);
				const t = setTimeout(() => { ws.close(); reject(); }, 1_000);
				ws.addEventListener("open", () => { clearTimeout(t); ws.close(); resolve(); });
				ws.addEventListener("error", () => { clearTimeout(t); reject(); });
			});
			log.success(`embedded gateway ready: ${url}`);
			return { url, token, proc };
		} catch {
			await new Promise((r) => setTimeout(r, 300));
		}
	}
	proc.kill("SIGTERM");
	throw new Error("nexal-gateway did not start within 10s");
}

async function main(): Promise<void> {
	const cfg = await loadConfig();
	const httpPort = Number(
		(cfg.channel.http?.port as number | string | undefined) ??
			process.env.NEXAL_HTTP_PORT ??
			"3000",
	);
	// Load saved auth & model config from settings DB (PGlite).
	await applySavedAuth();

	const provider = process.env.NEXAL_MODEL_PROVIDER ?? "openrouter";
	const modelId = process.env.NEXAL_MODEL ?? "openai/gpt-4o";
	log.info(`model: ${provider} / ${modelId}`);
	const coordinatorPrompt =
		process.env.NEXAL_COORDINATOR_SYSTEM_PROMPT ?? DEFAULT_COORDINATOR_PROMPT;
	const executorPrompt =
		process.env.NEXAL_EXECUTOR_SYSTEM_PROMPT ?? DEFAULT_EXECUTOR_PROMPT;

	let gatewayUrl = process.env.NEXAL_GATEWAY_URL ?? cfg.gateway.url;
	let gatewayUnix: string | undefined = process.env.NEXAL_GATEWAY_UNIX ?? (cfg.gateway as any).unix;
	let gatewayToken = process.env.NEXAL_GATEWAY_TOKEN ?? cfg.gateway.token;
	let gatewayProc: Subprocess | null = null;

	if (!gatewayToken) {
		// Auto-start an embedded gateway for local dev.
		const launched = await launchGateway();
		gatewayUrl = launched.url;
		gatewayToken = launched.token;
		gatewayProc = launched.proc;
	}

	const gateway = new GatewayClient({
		url: gatewayUrl,
		unix: gatewayUnix,
		token: gatewayToken,
		clientName: cfg.gateway.clientName,
	});
	await gateway.hello();
	log.info(`gateway connected: ${gatewayUnix ? `unix:${gatewayUnix}` : gatewayUrl}`);

	const sandbox = createSandboxBackend({
		gatewayClient: gateway,
		gatewayOptions: {},
	});
	log.info(`sandbox backend: ${sandbox.name} (workers only)`);

	const model = getModel(provider as any, modelId);

	const channels = new Map<string, Channel>();
	channels.set("http", new HttpChannel({ port: httpPort }));

	const wsBucket = cfg.channel.ws ?? {};
	const wsPort = Number(wsBucket.port ?? process.env.NEXAL_WS_PORT ?? "3001");
	channels.set(
		"ws",
		new WsChannel({
			port: wsPort,
			host: (wsBucket.host as string | undefined) ?? "127.0.0.1",
		}),
	);

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

	// Worker registry — long-lived persistent workers + one-shot tasks
	// spawned by the dispatcher. Persistence via Drizzle on Postgres
	// (Bun.sql native driver); containers survive nexal process restart
	// so live workers resume automatically.
	const workerStore = await createWorkerStore({ url: cfg.workers.url });
	log.info(`worker store ready (maxConcurrent=${cfg.workers.maxConcurrent})`);
	// `WorkerRegistry` is constructed BEFORE the factories close over it
	// because the coordinator factory recursively builds dispatcher
	// tools that reference the same registry — sub-coordinators can
	// spawn more agents through it. Explicit type annotation breaks
	// the inference cycle.
	// Forward decl so `pool` can be referenced from deliverToTopLevel
	// before it's constructed below.
	let pool: AgentPool | undefined;

	const workers: WorkerRegistry = new WorkerRegistry({
		store: workerStore,
		sandbox,
		model,
		modelProvider: provider,
		modelId,
		channels,
		maxConcurrent: cfg.workers.maxConcurrent,
		executorSystemPromptDefault: executorPrompt,
		coordinatorSystemPromptDefault: coordinatorPrompt,
		gateway,
		executorProxies: cfg.executor.proxies,
		executorTools: (runner) => {
			const client = runner.execClient;
			const tools: AgentTool<any>[] = [
				createSendUpdateTool(runner),
				createReportToParentTool(workers, runner),
			];
			if (client) tools.unshift(createBashTool(client));
			else log.error(`executor ${runner.id} has no exec client`);
			return tools;
		},
		coordinatorTools: (runner) => [
			// Sub-coordinator: same dispatcher surface as the top-level
			// one, scoped to its own subtree (its row id becomes the
			// parentSessionKey for any agents it spawns).
			...createDispatcherTools(workers, {
				parentSessionKey: runner.id,
				sourceChannel: runner.row.sourceChannel,
				sourceChatId: runner.row.sourceChatId,
				sourceReplyTo: runner.row.sourceReplyTo ?? null,
			}),
			// And the upward edge: sub-coordinators can escalate to
			// their own parent (which may be another sub-coordinator
			// or the top-level coordinator).
			createReportToParentTool(workers, runner),
		],
		deliverToTopLevel: (sessionKey, sender, message) => {
			if (!pool) {
				log.error("deliverToTopLevel before pool ready");
				return;
			}
			pool.injectMessage(sessionKey, sender, message);
		},
	});

	pool = new AgentPool({
		systemPrompt: coordinatorPrompt,
		model,
		tools: [],
		toolsFor: async (key) => {
			// Top-level coordinator: NO sandbox, NO bash. Just the
			// dispatcher tool surface scoped to this chat.
			const sepIdx = key.indexOf(":");
			const channelName = sepIdx === -1 ? key : key.slice(0, sepIdx);
			const chatId = sepIdx === -1 ? "" : key.slice(sepIdx + 1);
			return {
				tools: createDispatcherTools(workers, {
					parentSessionKey: key,
					sourceChannel: channelName,
					sourceChatId: chatId,
				}),
				// no dispose: nothing to release
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
	let shuttingDown = false;
	const shutdown = async (sig: string) => {
		if (shuttingDown) return;
		shuttingDown = true;
		log.error(`${sig} received, shutting down`);
		stop.abort();
		await pool.shutdown();
		// Suspend workers BEFORE releaseAll: suspend calls sandbox.detach()
		// which keeps worker containers running so they resume on next
		// startup; releaseAll then has nothing left to clean up.
		await workers.shutdown().catch((err) =>
			log.error("worker registry shutdown", err),
		);
		await Promise.all([...channels.values()].map((c) => c.stop().catch(() => undefined)));
		await sandbox.releaseAll();
		await closeSettings().catch(() => undefined);
		if (gatewayProc) {
			gatewayProc.kill("SIGTERM");
			log.error("embedded gateway stopped");
		}
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
					log.error(`channel=${channel.name} dispatch error`, err);
				}
			}),
		),
	);

	// Resume non-terminal workers after channels are up so their
	// send_update calls can land on the right destination.
	await workers.resumePending().catch((err: unknown) =>
		log.error("resumePending failed", err),
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
	log.error("fatal", err);
	process.exit(1);
});
