/**
 * Nexal entry — load config, connect to nexal-gateway, start channels,
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
import type { AgentTool } from "@mariozechner/pi-agent-core";
import { getModel } from "@mariozechner/pi-ai";

import { AgentPool } from "./agent-pool.ts";
import type { Channel } from "./channels/types.ts";
import { HttpChannel } from "./channels/http.ts";
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
import { createWorkerStore } from "./workers/store.ts";

const DEFAULT_COORDINATOR_PROMPT = [
	"You are a Nexal coordinator. You DO NOT execute tasks yourself — you have no shell, no filesystem, no network.",
	"You schedule work onto agents below you and route messages between them.",
	"For every incoming message, decide:",
	"  1. Does an existing agent (use list_agents) already own this domain? If yes, route_to_agent(id, message).",
	"  2. Is this an ongoing project / role / area? Spawn an executor (spawn_executor) with a clear system_prompt that defines its identity.",
	"  3. Is this a one-shot job (single command, single fetch, single build)? Use spawn_shot_task.",
	"  4. Is the domain large enough to deserve its own scheduling layer? Spawn a sub-coordinator (spawn_coordinator) and route work to it.",
	"Executors reply to the user directly via send_update — you don't need to summarize their output. Keep your own replies short: announce routing decisions, ask for clarification when ambiguous, but never try to do the work yourself.",
].join("\n");

const DEFAULT_EXECUTOR_PROMPT = [
	"You are a Nexal executor agent. You have bash inside a Podman sandbox at /workspace and one tool to talk to the user: send_update.",
	"Filesystem layout:",
	"  - /workspace        — user-facing project area. Put files the user expects to see here.",
	"  - /workspace/.nexal — your HOME and scratch space (logs, lockfiles, dotfiles, internal state). $HOME and $NEXAL_DATA_DIR both point here.",
	"Do the work assigned to you. Use bash freely. Call send_update for milestones, when you need clarification, and to deliver final results.",
	"Do NOT echo every intermediate thought — each send_update call becomes a separate Telegram message.",
].join("\n");

async function main(): Promise<void> {
	const cfg = await loadConfig();
	const httpPort = Number(
		(cfg.channel.http?.port as number | string | undefined) ??
			process.env.NEXAL_HTTP_PORT ??
			"3000",
	);
	const provider = process.env.NEXAL_MODEL_PROVIDER ?? "openrouter";
	const modelId = process.env.NEXAL_MODEL ?? "openai/gpt-4o";
	const coordinatorPrompt =
		process.env.NEXAL_COORDINATOR_SYSTEM_PROMPT ?? DEFAULT_COORDINATOR_PROMPT;
	const executorPrompt =
		process.env.NEXAL_EXECUTOR_SYSTEM_PROMPT ?? DEFAULT_EXECUTOR_PROMPT;

	const gatewayUrl = process.env.NEXAL_GATEWAY_URL ?? cfg.gateway.url;
	const gatewayToken = process.env.NEXAL_GATEWAY_TOKEN ?? cfg.gateway.token;
	if (!gatewayToken) {
		throw new Error(
			"no nexal-gateway token configured; set [gateway].token in ~/.nexal/config.toml or NEXAL_GATEWAY_TOKEN",
		);
	}
	const gateway = new GatewayClient({
		url: gatewayUrl,
		token: gatewayToken,
		clientName: cfg.gateway.clientName,
	});
	await gateway.hello();
	console.log(`[nexal] gateway connected: ${gatewayUrl}`);

	const sandbox = createSandboxBackend({
		gatewayClient: gateway,
		gatewayOptions: { defaultWorkspace: cfg.workspace },
	});
	console.log(`[nexal] sandbox backend: ${sandbox.name} (workers only)`);

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

	// Worker registry — long-lived persistent workers + one-shot tasks
	// spawned by the dispatcher. Persistence via Drizzle (SQLite or
	// Postgres); containers survive nexal process restart so live
	// workers resume automatically.
	const workerStore = await createWorkerStore({
		backend: cfg.workers.backend,
		url: cfg.workers.url,
	});
	console.log(
		`[nexal] worker store: ${workerStore.backend} (maxConcurrent=${cfg.workers.maxConcurrent})`,
	);
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
		executorTools: (runner) => {
			const client = runner.execClient;
			const tools: AgentTool<any>[] = [
				createSendUpdateTool(runner),
				createReportToParentTool(workers, runner),
			];
			if (client) tools.unshift(createBashTool(client));
			else console.error(`[nexal] executor ${runner.id} has no exec client`);
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
				console.error("[nexal] deliverToTopLevel before pool ready");
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
	const shutdown = async (sig: string) => {
		console.error(`[nexal] ${sig} received, shutting down`);
		stop.abort();
		await pool.shutdown();
		// Suspend workers BEFORE releaseAll: suspend calls sandbox.detach()
		// which keeps worker containers running so they resume on next
		// startup; releaseAll then has nothing left to clean up.
		await workers.shutdown().catch((err) =>
			console.error("[nexal] worker registry shutdown", err),
		);
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

	// Resume non-terminal workers after channels are up so their
	// send_update calls can land on the right destination.
	await workers.resumePending().catch((err: unknown) =>
		console.error("[nexal] resumePending failed", err),
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
