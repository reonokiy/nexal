/**
 * nexal entry — load config, connect to nexal-gateway, start channels,
 * wire them into the AgentPool.
 */
import { parseArgs } from "node:util";
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
import { createBashTool } from "./tools/bash.ts";
import { createReportToParentTool } from "./tools/report_to_parent.ts";
import { createSendUpdateTool } from "./tools/send_update.ts";
import { createCoordinatorTools } from "./tools/worker.ts";
import { WorkerRegistry } from "./workers/registry.ts";
import { loadAuth, loadModelConfig, closeSettings } from "./settings.ts";
import { createWorkerStore } from "./workers/store.ts";
import { CommandRegistry } from "./commands/registry.ts";
import { registerBuiltins } from "./commands/builtin.ts";
import {
	isCompiled,
	COORDINATOR_PROMPT,
	EXECUTOR_PROMPT,
	embeddedGatewayPath,
	embeddedAgentPath,
	extractEmbeddedBinaries,
} from "./embedded.ts";

const DEFAULT_COORDINATOR_PROMPT = COORDINATOR_PROMPT;
const DEFAULT_EXECUTOR_PROMPT = EXECUTOR_PROMPT;

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
						log.info(`${providerName} OAuth token was expired, refreshed and persisted new credentials`);
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
		log.error("failed to load saved auth, continuing without credentials:", err);
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

	// Resolve binary paths: compiled mode uses extracted embedded binaries,
	// dev mode reads from target/release/.
	let gatewayBin: string;
	let agentBin: string | null;

	if (isCompiled) {
		const extracted = await extractEmbeddedBinaries();
		if (!extracted.gatewayBin) {
			throw new Error("nexal-gateway was not embedded in this binary — rebuild with `just compile`");
		}
		gatewayBin = extracted.gatewayBin;
		agentBin = extracted.agentBin;
	} else {
		const projectRoot = join(import.meta.dir, "..");
		gatewayBin = join(projectRoot, "target/release/nexal-gateway");
		agentBin = join(projectRoot, "target/release/nexal-agent");
		if (!existsSync(gatewayBin)) {
			throw new Error(
				`nexal-gateway binary not found at ${gatewayBin} — run 'cargo build --release -p nexal-gateway' first`,
			);
		}
		if (!existsSync(agentBin)) agentBin = null;
	}

	log.info(`no gateway token configured, auto-starting embedded gateway from ${gatewayBin}`);

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
			...(agentBin ? ["--agent-bin", agentBin] : []),
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
			log.success(`embedded gateway ready at ${url}`);
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
	log.info(`using model ${modelId} via ${provider}`);
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
	log.info(`connected to gateway at ${gatewayUnix ? gatewayUnix : gatewayUrl} as "${cfg.gateway.clientName}"`);

	const model = getModel(provider as any, modelId);

	const commands = new CommandRegistry();
	registerBuiltins(commands);

	const channels = new Map<string, Channel>();
	channels.set("http", new HttpChannel({ port: httpPort, commands }));

	const wsBucket = cfg.channel.ws ?? {};
	const wsPort = Number(wsBucket.port ?? process.env.NEXAL_WS_PORT ?? "3001");
	channels.set(
		"ws",
		new WsChannel({
			port: wsPort,
			host: (wsBucket.host as string | undefined) ?? "127.0.0.1",
			commands,
		}),
	);

	const tgBucket = cfg.channel.telegram ?? {};
	const tgToken =
		(tgBucket.botToken as string | undefined) ??
		process.env.TELEGRAM_BOT_TOKEN ??
		process.env.NEXAL_TELEGRAM_BOT_TOKEN;
	if (tgToken && tgBucket.enabled === true) {
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
				commands,
			}),
		);
	}

	const hbCfg = cfg.channel.heartbeat ?? {};
	if (hbCfg.enabled === true) {
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
	if (cronCfg.enabled === true) {
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
	log.info(`worker store ready, up to ${cfg.workers.maxConcurrent} concurrent workers`);
	// `WorkerRegistry` is constructed BEFORE the factories close over it
	// because the coordinator factory recursively builds dispatcher
	// tools that reference the same registry — sub-coordinators can
	// spawn more agents through it. Explicit type annotation breaks
	// the inference cycle.
	// Forward decl so `pool` can be referenced from forwardToCoordinator
	// before it's constructed below.
	let pool: AgentPool | undefined;

	const workers: WorkerRegistry = new WorkerRegistry({
		store: workerStore,
		gateway,
		model,
		modelProvider: provider,
		modelId,
		channels,
		maxConcurrent: cfg.workers.maxConcurrent,
		executorSystemPromptDefault: executorPrompt,
		coordinatorSystemPromptDefault: coordinatorPrompt,
		executorProxies: cfg.executor.proxies,
		executorTools: (runner) => {
			const client = runner.execClient;
			const tools: AgentTool<any>[] = [
				createSendUpdateTool(runner),
				createReportToParentTool(workers, runner),
			];
			if (client) tools.unshift(createBashTool(client));
			else log.error(`executor "${runner.row.name}" has no exec client, bash tool will be unavailable`);
			return tools;
		},
		coordinatorTools: (runner) => [
			// Sub-coordinator: same dispatcher surface as the top-level
			// one, scoped to its own subtree (its row id becomes the
			// parentSessionKey for any agents it spawns).
			...createCoordinatorTools(workers, {
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
		forwardToCoordinator: (sessionKey, sender, content) => {
			if (!pool) {
				log.error(`cannot deliver message from "${sender}" to top-level coordinator, agent pool is not ready yet`);
				return;
			}
			pool.forwardChildReport(sessionKey, sender, content);
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
				tools: createCoordinatorTools(workers, {
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
		log.info(`${sig} received, shutting down gracefully`);
		stop.abort();
		await pool.shutdown();
		// Suspend workers BEFORE releaseAll: suspend calls sandbox.detach()
		// which keeps worker containers running so they resume on next
		// startup; releaseAll then has nothing left to clean up.
		await workers.shutdown().catch((err) =>
			log.error("worker registry shutdown failed, some workers may not have been suspended cleanly", err),
		);
		await Promise.all([...channels.values()].map((c) => c.stop().catch(() => undefined)));
		await gateway.releaseAllAgents();
		await closeSettings().catch(() => undefined);
		if (gatewayProc) {
			gatewayProc.kill("SIGTERM");
			log.info("stopped embedded gateway");
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
					log.error(`failed to dispatch incoming message from ${channel.name} channel`, err);
				}
			}),
		),
	);

	// Resume non-terminal workers after channels are up so their
	// send_update calls can land on the right destination.
	await workers.resumePending().catch((err: unknown) =>
		log.error("failed to resume workers from previous process", err),
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

export { main };
