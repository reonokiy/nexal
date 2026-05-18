/**
 * nexal entry — load config, connect to nexal-gateway, start channels,
 * wire them into the AgentPool.
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
import { ChannelManager } from "./channels/manager.ts";
import { loadConfig } from "./config.ts";
import { GatewayClient } from "./gateway/client.ts";
import { createBashTool } from "./tools/bash.ts";
import { createReportToParentTool } from "./tools/report_to_parent.ts";
import { createSendUpdateTool } from "./tools/send_update.ts";
import { createCoordinatorTools } from "./tools/worker.ts";
import { WorkerRegistry } from "./workers/registry.ts";
import { loadAuth, loadModelConfig, loadProviderConfig, loadAllToolApiKeys } from "./settings.ts";
import { setDbUrl, runMigrations, closeDb } from "./db.ts";
import { createWorkerStore } from "./workers/store.ts";
import { createTapeStore } from "./tape/store.ts";
import { createFileStore } from "./tape/file-store.ts";
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

// ── Model & auth from DB ─────────────────────────────────────────────
//
// Providers are totally configured in the database — no env vars needed
// for base URLs or API keys. The settings KV stores:
//   model:provider  → "opencode-go"
//   model:id        → "kimi-k2.6"
//   provider:<name> → { base_url, wire_api, thinking_mode }
//   auth:<name>     → { provider, apiKey }
//
// Fallback: if no DB config exists, pi-ai's built-in models + env vars
// are used (backward compatible).

interface ModelFromDb {
	model: import("@mariozechner/pi-ai").Model<any>;
	getApiKey: (provider: string) => Promise<string | undefined>;
}

async function buildModelFromDb(): Promise<ModelFromDb | null> {
	try {
		const saved = await loadModelConfig();
		if (!saved) return null;

		const providerCfg = await loadProviderConfig(saved.provider);
		const auth = await loadAuth(saved.provider);

		const baseUrl = providerCfg?.base_url ? String(providerCfg.base_url) : undefined;
		const wireApi = providerCfg?.wire_api ? String(providerCfg.wire_api) : "chat";

		if (baseUrl) {
			// DB has a custom provider config → build a synthetic Model.
			const model: import("@mariozechner/pi-ai").Model<"openai-completions"> = {
				id: `${saved.provider}/${saved.modelId}`,
				name: saved.modelId,
				api: "openai-completions",
				provider: saved.provider as any,
				baseUrl,
				reasoning: Boolean(providerCfg?.thinking_mode),
				input: ["text"],
				cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
				contextWindow: 128000,
				maxTokens: 8192,
			};
			return {
				model,
				getApiKey: async (p) => {
					const a = await loadAuth(p);
					return a?.apiKey;
				},
			};
		}

		// No custom base URL → try pi-ai's built-in model registry.
		const m = getModel(saved.provider as any, saved.modelId as any);
		if (m) {
			return {
				model: m,
				getApiKey: async (p) => {
					const a = await loadAuth(p);
					return a?.apiKey;
				},
			};
		}
		return null;
	} catch (err) {
		log.error("failed to build model from DB", err);
		return null;
	}
}

// ── Legacy: env-var fallback (kept for backward compat) ────────────

async function applySavedAuth(): Promise<void> {
	try {
		const saved = await loadModelConfig();
		if (saved) {
			if (!process.env.NEXAL_MODEL_PROVIDER) process.env.NEXAL_MODEL_PROVIDER = saved.provider;
			if (!process.env.NEXAL_MODEL) process.env.NEXAL_MODEL = saved.modelId;
		}

		const providerName = process.env.NEXAL_MODEL_PROVIDER ?? saved?.provider;
		if (!providerName) return;

		const auth = await loadAuth(providerName);
		if (!auth) return;

		const envKey = apiKeyEnvKey(providerName);
		if (envKey && !process.env[envKey]) {
			process.env[envKey] = auth.apiKey;
			log.info(`loaded saved ${providerName} API key`);
		}
	} catch (err) {
		log.error("failed to load saved auth, continuing without credentials:", err);
	}
}

// ── Tool API key bootstrap ─────────────────────────────────────────

async function applySavedToolKeys(): Promise<void> {
	try {
		const keys = await loadAllToolApiKeys();
		if (Object.keys(keys).length === 0) return;
		const envMap: Record<string, string> = {
			tavily: "TAVILY_API_KEY",
			jina: "JINA_API_KEY",
			gemini: "GEMINI_API_KEY",
		};
		for (const [name, apiKey] of Object.entries(keys)) {
			const envKey = envMap[name] ?? `${name.toUpperCase()}_API_KEY`;
			if (!process.env[envKey]) {
				process.env[envKey] = apiKey;
				log.info(`loaded tool API key for ${name}`);
			}
		}
	} catch (err) {
		log.error("failed to load tool API keys from DB", err);
	}
}

export function apiKeyEnvKey(provider: string): string | null {
	switch (provider) {
		case "openrouter": return "OPENROUTER_API_KEY";
		case "kimi-coding": return "KIMI_API_KEY";
		case "deepseek": return "DEEPSEEK_API_KEY";
		case "opencode-go": return "OPENCODE_API_KEY";
		// kept for users who still have these set in env / config:
		case "anthropic": return "ANTHROPIC_API_KEY";
		case "openai": return "OPENAI_API_KEY";
		case "google": return "GEMINI_API_KEY";
		case "mistral": return "MISTRAL_API_KEY";
		default: return null;
	}
}

// ── Embedded gateway for local dev ──────────────────────────────────

async function launchGateway(): Promise<{
	url: string;
	accessKey: string;
	secretKey: string;
	proc: Subprocess;
}> {
	const accessKey = crypto.randomUUID();
	const secretKey = crypto.randomUUID();
	const url = "https://127.0.0.1:15500";
	const proxyUrl = "http://127.0.0.1:15501";

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

	log.info(`no gateway credentials configured, auto-starting embedded gateway from ${gatewayBin}`);

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
			"--listen", "127.0.0.1:15500",
			"--proxy-listen", "127.0.0.1:15501",
			...(agentBin ? ["--agent-bin", agentBin] : []),
		],
		stdout: "inherit",
		stderr: "inherit",
		env: {
			...process.env,
			NEXAL_LOG: process.env.NEXAL_LOG ?? "info",
			NEXAL_GATEWAY_ACCESS_KEY: accessKey,
			NEXAL_GATEWAY_SECRET_KEY: secretKey,
		},
	});

	// Poll the plain-HTTP proxy port — it comes up alongside the WebTransport
	// listener and any TCP response (even 404) means the gateway is ready.
	const deadline = Date.now() + 10_000;
	while (Date.now() < deadline) {
		try {
			const ctrl = new AbortController();
			const t = setTimeout(() => ctrl.abort(), 800);
			await fetch(proxyUrl, { signal: ctrl.signal });
			clearTimeout(t);
			log.success(`embedded gateway ready at ${url}`);
			return { url, accessKey, secretKey, proc };
		} catch {
			await new Promise((r) => setTimeout(r, 300));
		}
	}
	proc.kill("SIGTERM");
	throw new Error("nexal-gateway did not start within 10s");
}

async function main(): Promise<void> {
	const cfg = await loadConfig();
	// Open the shared Postgres connection and apply migrations before
	// anything reads the DB. No embedded fallback — fail fast with a
	// clear message if no Postgres URL is configured.
	setDbUrl(cfg.workers.url);
	try {
		await runMigrations();
	} catch (err) {
		log.error(String(err instanceof Error ? err.message : err));
		process.exit(1);
	}

	// Load model + auth from DB. Falls back to env vars if DB is empty.
	const dbModel = await buildModelFromDb();
	if (!dbModel) {
		// Legacy path: DB has no config → try env vars.
		await applySavedAuth();
	}
	const provider = dbModel
		? (dbModel.model.provider as string)
		: (process.env.NEXAL_MODEL_PROVIDER ?? "openrouter");
	const modelId = dbModel
		? dbModel.model.id
		: (process.env.NEXAL_MODEL ?? "openai/gpt-4o");
	const getApiKeyFromDb = dbModel?.getApiKey;
	const model = dbModel?.model ?? getModel(provider as any, modelId as any);
	log.info(`using model ${modelId} via ${provider}`);

	// Load external tool API keys from DB (Tavily, Jina, Gemini, …).
	await applySavedToolKeys();
	const coordinatorPrompt =
		process.env.NEXAL_COORDINATOR_SYSTEM_PROMPT ?? DEFAULT_COORDINATOR_PROMPT;
	const executorPrompt =
		process.env.NEXAL_EXECUTOR_SYSTEM_PROMPT ?? DEFAULT_EXECUTOR_PROMPT;

	let gatewayUrl = process.env.NEXAL_GATEWAY_URL ?? cfg.gateway.url;
	let gatewayUnix: string | undefined = process.env.NEXAL_GATEWAY_UNIX ?? (cfg.gateway as any).unix;
	let gatewayAccessKey = process.env.NEXAL_GATEWAY_ACCESS_KEY ?? cfg.gateway.accessKey;
	let gatewaySecretKey = process.env.NEXAL_GATEWAY_SECRET_KEY ?? cfg.gateway.secretKey;
	let gatewayProc: Subprocess | null = null;

	if (!gatewayAccessKey || !gatewaySecretKey) {
		// Auto-start an embedded gateway for local dev.
		const launched = await launchGateway();
		gatewayUrl = launched.url;
		gatewayAccessKey = launched.accessKey;
		gatewaySecretKey = launched.secretKey;
		gatewayProc = launched.proc;
	}

	const gateway = new GatewayClient({
		url: gatewayUrl,
		unix: gatewayUnix,
		accessKey: gatewayAccessKey,
		secretKey: gatewaySecretKey,
		clientName: cfg.gateway.clientName,
	});
	await gateway.hello();
	log.info(`connected to gateway at ${gatewayUnix ? gatewayUnix : gatewayUrl} as "${cfg.gateway.clientName}"`);

	// Channel config lives exclusively in the DB (settings KV). The
	// manager (created after `pool`) constructs/starts channels from it
	// and hot-reloads on every saveChannelConfig/deleteChannelConfig
	// write. This Map is shared by reference with WorkerRegistry &
	// AgentPool below — mutating it in place keeps reply routing correct.
	const channels = new Map<string, Channel>();

	// Worker registry — long-lived persistent workers + one-shot tasks
	// spawned by the dispatcher. Persistence via Drizzle on Postgres
	// (Bun.sql native driver); containers survive nexal process restart
	// so live workers resume automatically.
	const workerStore = await createWorkerStore({ url: cfg.workers.url });
	log.info(`worker store ready, up to ${cfg.workers.maxConcurrent} concurrent workers`);

	// Tape store — persistent conversation history (AgentPool + workers).
	const tapeStore = createTapeStore();
	const fileStore = createFileStore(cfg.storage);
	log.info(`tape store ready (storage provider: ${cfg.storage.provider})`);
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
		tapeStore,
		getApiKey: getApiKeyFromDb,
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
		tapeStore,
		getApiKey: getApiKeyFromDb,
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

	const manager = new ChannelManager({
		channels,
		gateway,
		onMessage: (msg) => {
			try {
				pool!.handle(msg);
			} catch (err) {
				log.error(`failed to dispatch incoming message from ${msg.channel} channel`, err);
			}
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
		await manager.stopAll();
		await gateway.releaseAllAgents();
		await closeDb().catch(() => undefined);
		if (gatewayProc) {
			gatewayProc.kill("SIGTERM");
			log.info("stopped embedded gateway");
		}
		process.exit(0);
	};
	process.on("SIGINT", () => void shutdown("SIGINT"));
	process.on("SIGTERM", () => void shutdown("SIGTERM"));

	// Initial reconcile builds & starts every DB-configured channel and
	// wires the change-notification + 5s poll loop for hot-reload.
	await manager.startInitial();

	// Resume non-terminal workers after channels are up so their
	// send_update calls can land on the right destination.
	await workers.resumePending().catch((err: unknown) =>
		log.error("failed to resume workers from previous process", err),
	);

	await new Promise<void>((resolve) => {
		stop.signal.addEventListener("abort", () => resolve());
	});
}

export { main };
