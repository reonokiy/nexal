/**
 * nexal entry — load config, connect to nexal-gateway, start channels,
 * wire them into the AgentPool.
 */
import type { AgentTool } from "@mariozechner/pi-agent-core";
import { log } from "./log.ts";
import { getModel } from "@mariozechner/pi-ai";

import { AgentPool } from "./agent-pool.ts";
import type { Channel } from "./channels/types.ts";
import { ChannelManager } from "./channels/manager.ts";
import { loadConfig } from "./config.ts";
import { GatewayClient } from "./gateway/index.ts";
import { createMessageSender } from "./messaging/index.ts";
import { createBashTool } from "./tools/bash.ts";
import { createReportToParentTool } from "./tools/report_to_parent.ts";
import { createSendUpdateTool } from "./tools/send_update.ts";
import { createCoordinatorTools } from "./tools/coordinator/index.ts";
import { WorkerRegistry } from "./workers/registry.ts";
import { loadAuth, loadModelConfig, loadProviderConfig } from "./settings.ts";
import { setDbUrl, runMigrations, closeDb } from "./db.ts";
import { createWorkerStore } from "./workers/store.ts";
import { createTapeStore, createFileStore, getOrCreateSessionTapeRef, getSessionTapeRef } from "./tape/index.ts";
import COORDINATOR_PROMPT from "./prompts/coordinator.md" with { type: "text" };
import EXECUTOR_PROMPT from "./prompts/executor.md" with { type: "text" };

// ── Model & auth from DB ─────────────────────────────────────────────
//
// Providers are configured exclusively through the database — the
// settings KV stores:
//   model:provider  → "opencode-go"
//   model:id        → "kimi-k2.6"
//   provider:<name> → { base_url, wire_api, thinking_mode }
//   auth:<name>     → { provider, apiKey }

interface ModelFromDb {
	model: import("@mariozechner/pi-ai").Model<any>;
	getApiKey: (provider: string) => Promise<string | undefined>;
}

async function resolveApiKey(provider: string): Promise<string | undefined> {
	return (await loadAuth(provider))?.apiKey;
}

async function buildModelFromDb(): Promise<ModelFromDb | null> {
	try {
		const saved = await loadModelConfig();
		if (!saved) return null;

		const providerCfg = await loadProviderConfig(saved.provider);
		const baseUrl = providerCfg?.base_url ? String(providerCfg.base_url) : undefined;

		if (baseUrl) {
			const model: import("@mariozechner/pi-ai").Model<"openai-completions"> = {
				id: saved.modelId,
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
			return { model, getApiKey: resolveApiKey };
		}

		const m = getModel(saved.provider as any, saved.modelId as any);
		if (m) return { model: m, getApiKey: resolveApiKey };
		return null;
	} catch (err) {
		log.error("failed to build model from DB", err);
		return null;
	}
}

async function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		return await Promise.race([
			promise,
			new Promise<T>((_, reject) => {
				timer = setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms);
			}),
		]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}

async function main(): Promise<void> {
	const cfg = await loadConfig();
	// Open the shared Postgres connection and apply migrations before
	// anything reads the DB. Fail fast with a clear message if no
	// Postgres URL is configured.
	setDbUrl(cfg.workers.url);
	try {
		await runMigrations();
	} catch (err) {
		log.error(String(err instanceof Error ? err.message : err));
		process.exit(1);
	}

	// Load model + auth from DB. If no model is configured yet, still
	// start the always-on channels so the web UI / CLI can authenticate
	// and use /model, /apikey, and /settings to bootstrap the server.
	const dbModel = await buildModelFromDb();
	if (!dbModel) {
		log.warn(
			"no model configured in DB — server will accept connections, but chat and workers are disabled " +
				"until model:provider, model:id, and auth:<provider> are configured then nexal is restarted.",
		);
	} else {
		log.info(`using model ${dbModel.model.id} via ${dbModel.model.provider as string}`);
	}

	const gatewayUrl = process.env.NEXAL_GATEWAY_URL ?? cfg.gateway.url;
	const gatewayAccessKey = process.env.NEXAL_GATEWAY_ACCESS_KEY ?? cfg.gateway.accessKey;
	const gatewaySecretKey = process.env.NEXAL_GATEWAY_SECRET_KEY ?? cfg.gateway.secretKey;

	if (!gatewayAccessKey || !gatewaySecretKey) {
		log.warn(
			"no gateway credentials configured — sandbox workers will be unavailable. " +
				"Start nexal-gateway manually and set NEXAL_GATEWAY_URL / NEXAL_GATEWAY_ACCESS_KEY / " +
				"NEXAL_GATEWAY_SECRET_KEY (or the equivalent [gateway] entries in ~/.nexal/config.toml).",
		);
	}

	// ── Gateway connection (best-effort, does not block startup) ──────

	const gateway = new GatewayClient({
		url: gatewayUrl,
		accessKey: gatewayAccessKey,
		secretKey: gatewaySecretKey,
		clientName: cfg.gateway.clientName,
	});
	try {
		await withTimeout(gateway.hello(), 5_000, "gateway hello");
		log.info(`connected to gateway at ${gatewayUrl}`);
	} catch (err) {
		log.warn(`gateway hello failed — sandbox workers unavailable: ${err instanceof Error ? err.message : err}`);
	}

	// Channel config lives exclusively in the DB (settings KV). The
	// manager (created after `pool`) constructs/starts channels from it
	// and hot-reloads on every saveChannelConfig/deleteChannelConfig
	// write. This Map is shared by reference with WorkerRegistry &
	// AgentPool below — mutating it in place keeps reply routing correct.
	const channels = new Map<string, Channel>();
	const sender = createMessageSender(channels);

	let workerStore: Awaited<ReturnType<typeof createWorkerStore>> | undefined;
	let tapeStore: ReturnType<typeof createTapeStore> | undefined;

	if (dbModel) {
		// Worker registry — long-lived persistent workers + one-shot tasks
		// spawned by the dispatcher. Persistence via Drizzle on Postgres
		// (Bun.sql native driver); containers survive nexal process restart
		// so live workers resume automatically.
		workerStore = await createWorkerStore({ url: cfg.workers.url });
		log.info(`worker store ready, up to ${cfg.workers.maxConcurrent} concurrent workers`);

		// Tape store — persistent conversation history (AgentPool + workers).
		const fileStore = createFileStore(cfg.storage);
		tapeStore = createTapeStore({
			fileStore,
			maxInlineSize: cfg.storage.maxInlineSize,
		});
		log.info(`tape store ready (storage provider: ${cfg.storage.provider})`);
	}
	// `WorkerRegistry` is constructed BEFORE the factories close over it
	// because the coordinator factory recursively builds dispatcher
	// tools that reference the same registry — sub-coordinators can
	// spawn more agents through it. Explicit type annotation breaks
	// the inference cycle.
	// Forward decl so `pool` can be referenced from forwardToCoordinator
	// before it's constructed below.
	let pool: AgentPool | undefined;

	let workers: WorkerRegistry | undefined;
	if (dbModel && workerStore && tapeStore) {
		const { model, getApiKey: getApiKeyFromDb } = dbModel;
		const provider = model.provider as string;
		const modelId = model.id;

		workers = new WorkerRegistry({
			store: workerStore,
			gateway,
			model,
			modelProvider: provider,
			modelId,
			sender,
			maxConcurrent: cfg.workers.maxConcurrent,
			tapeStore,
			getApiKey: getApiKeyFromDb,
			executorSystemPromptDefault: EXECUTOR_PROMPT,
			coordinatorSystemPromptDefault: COORDINATOR_PROMPT,
			executorProxies: cfg.executor.proxies,
			executorTools: (runner) => {
				const client = runner.execClient;
				const tools: AgentTool<any>[] = [
					createSendUpdateTool(runner),
					createReportToParentTool(workers!, runner),
				];
				if (client) tools.unshift(createBashTool(client));
				else log.error(`executor "${runner.row.name}" has no exec client, bash tool will be unavailable`);
				return tools;
			},
			coordinatorTools: (runner) => [
				// Sub-coordinator: same dispatcher surface as the top-level
				// one, scoped to its own subtree (its row id becomes the
				// parentSessionKey for any agents it spawns).
				...createCoordinatorTools(workers!, {
					parentSessionKey: runner.id,
					sourceChannel: runner.row.sourceChannel,
					sourceChatId: runner.row.sourceChatId,
					sourceReplyTo: runner.row.sourceReplyTo ?? null,
				}),
				// And the upward edge: sub-coordinators can escalate to
				// their own parent (which may be another sub-coordinator
				// or the top-level coordinator).
				createReportToParentTool(workers!, runner),
			],
			forwardToCoordinator: (sessionKey, sender, content) => {
				if (!pool) {
					log.error(`cannot deliver message from "${sender}" to top-level coordinator, agent pool is not ready yet`);
					return;
				}
				pool.forwardChildReport(sessionKey, sender, content);
			},
			getSessionTapeRef: getOrCreateSessionTapeRef,
		});

		pool = new AgentPool({
			systemPrompt: COORDINATOR_PROMPT,
			model,
			tools: [],
			tapeStore,
			getTapeRef: getOrCreateSessionTapeRef,
			getApiKey: getApiKeyFromDb,
			toolsFor: async (key) => {
				// Top-level coordinator: NO sandbox, NO bash. Just the
				// dispatcher tool surface scoped to this chat.
				const sepIdx = key.indexOf(":");
				const channelName = sepIdx === -1 ? key : key.slice(0, sepIdx);
				const chatId = sepIdx === -1 ? "" : key.slice(sepIdx + 1);
				return {
					tools: createCoordinatorTools(workers!, {
						parentSessionKey: key,
						sourceChannel: channelName,
						sourceChatId: chatId,
					}),
					// no dispose: nothing to release
				};
			},
			sender,
			debounce: {
				debounceMs: cfg.debounceSecs * 1_000,
				delayMs: cfg.messageDelaySecs * 1_000,
				activeWindowMs: cfg.activeWindowSecs * 1_000,
			},
		});
	}

	const manager = new ChannelManager({
		channels,
		gateway,
		tapeStore,
		getTapeRef: getSessionTapeRef,
		onMessage: (msg) => {
			try {
				if (pool) {
					pool.handle(msg);
					return;
				}
				void sender.send(msg.channel, {
					chatId: msg.chatId,
					text:
						"Model is not configured yet. Use /configure <provider> <model_id> <api_key> " +
						"(or the Settings page) to configure a provider, then restart nexal.",
				}).catch(() => undefined);
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
		await pool?.shutdown();
		// Suspend workers BEFORE releaseAll: suspend calls sandbox.detach()
		// which keeps worker containers running so they resume on next
		// startup; releaseAll then has nothing left to clean up.
		await workers?.shutdown().catch((err) =>
			log.error("worker registry shutdown failed, some workers may not have been suspended cleanly", err),
		);
		await manager.stopAll();
		await gateway.releaseAllAgents();
		await closeDb().catch(() => undefined);
		process.exit(0);
	};
	process.on("SIGINT", () => void shutdown("SIGINT"));
	process.on("SIGTERM", () => void shutdown("SIGTERM"));

	// Initial reconcile builds & starts every DB-configured channel and
	// wires the change-notification + 5s poll loop for hot-reload.
	await manager.startInitial();

	// Resume non-terminal workers after channels are up so their
	// send_update calls can land on the right destination.
	await workers?.resumePending().catch((err: unknown) =>
		log.error("failed to resume workers from previous process", err),
	);

	await new Promise<void>((resolve) => {
		stop.signal.addEventListener("abort", () => resolve());
	});
}

export { main };
