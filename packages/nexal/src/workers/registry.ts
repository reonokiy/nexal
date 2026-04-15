/**
 * WorkerRegistry — owns the live `WorkerRunner` set, manages the
 * spawn/route/cancel surface, and drives startup resume + shutdown
 * suspend.
 *
 * External API (consumed by `tools/worker.ts`):
 *   spawn(req)            — create a new persistent or shot worker
 *   route(id, message)    — feed a new instruction to a persistent worker
 *   cancel(id)            — abort + mark cancelled
 *   get(id)               — raw store lookup
 *   listForParent(key)    — recent workers for the parent chat
 *
 * Startup (from `index.ts`):
 *   resumePending()       — re-attach every non-terminal row
 *
 * Shutdown (from `index.ts`):
 *   shutdown()            — suspend every runner, persist, detach sandboxes
 *
 * Concurrency: `maxConcurrent` caps the number of *alive* runners
 * (each holds a Podman container). When the cap is hit, `spawn`
 * returns the queued row and the runner starts as soon as a slot
 * frees. Persistent workers stay alive forever (until cancel/shutdown)
 * so heavy use of `spawn_worker` will eventually need explicit
 * `cancel_worker` calls.
 */
import { randomUUID } from "node:crypto";
import type { AgentTool } from "@mariozechner/pi-agent-core";
import type { Model } from "@mariozechner/pi-ai";

import type { Channel } from "../channels/types.ts";
import type { SandboxBackend } from "../sandbox/types.ts";
import { WorkerRunner } from "./runner.ts";
import type {
	SendPolicy,
	WorkerKind,
	WorkerLifetime,
	WorkerRow,
	WorkerStore,
} from "./store.ts";

export interface WorkerRegistryConfig {
	store: WorkerStore;
	sandbox: SandboxBackend;
	model: Model<any>;
	modelProvider: string;
	modelId: string;
	channels: Map<string, Channel>;
	maxConcurrent: number;
	/** Default system prompt for executors with no override. */
	executorSystemPromptDefault: string;
	/** Default system prompt for sub-coordinators with no override. */
	coordinatorSystemPromptDefault: string;
	/** Tool factory for executor workers (typically bash + send_update + report_to_parent). */
	executorTools: (runner: WorkerRunner) => AgentTool<any>[];
	/** Tool factory for sub-coordinators (dispatcher tools + report_to_parent). */
	coordinatorTools: (runner: WorkerRunner) => AgentTool<any>[];
	/**
	 * Wake up the top-level coordinator (no DB row, lives in AgentPool)
	 * with a message from one of its direct children. `sessionKey` is
	 * the chat session key (`"<channel>:<chatId>"`); `sender` identifies
	 * the reporting child (typically `"worker:<id>"` or its name).
	 */
	deliverToTopLevel?: (sessionKey: string, sender: string, message: string) => void | Promise<void>;
}

export interface SpawnRequest {
	kind: WorkerKind;
	lifetime: WorkerLifetime;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
	name: string;
	/** Required for shot lifetime; optional for persistent. */
	initialPrompt?: string | null;
	systemPrompt?: string;
	sendPolicy?: SendPolicy;
}

export class WorkerRegistry {
	private readonly runners = new Map<string, WorkerRunner>();
	private readonly queue: string[] = [];
	private shuttingDown = false;

	constructor(private readonly cfg: WorkerRegistryConfig) {}

	async spawn(req: SpawnRequest): Promise<WorkerRow> {
		if (req.kind === "coordinator" && req.lifetime !== "persistent") {
			throw new Error("coordinators must have persistent lifetime");
		}
		if (req.lifetime === "shot" && !req.initialPrompt) {
			throw new Error("shot workers require an initial_prompt");
		}
		const id = randomUUID();
		const containerName = `nexal-worker-${id.replace(/-/g, "").slice(0, 12)}`;
		const defaultPrompt =
			req.kind === "coordinator"
				? this.cfg.coordinatorSystemPromptDefault
				: this.cfg.executorSystemPromptDefault;
		const row = await this.cfg.store.insert({
			id,
			kind: req.kind,
			lifetime: req.lifetime,
			parentSessionKey: req.parentSessionKey,
			sourceChannel: req.sourceChannel,
			sourceChatId: req.sourceChatId,
			sourceReplyTo: req.sourceReplyTo ?? null,
			name: req.name,
			initialPrompt: req.initialPrompt ?? null,
			systemPrompt: req.systemPrompt ?? defaultPrompt,
			modelProvider: this.cfg.modelProvider,
			modelId: this.cfg.modelId,
			containerName,
			sendPolicy: req.sendPolicy ?? "explicit",
		});
		this.queue.push(id);
		this.pump();
		return row;
	}

	/**
	 * Route a new user instruction to an existing persistent agent
	 * (coordinator or executor-persistent). If the agent is still
	 * queued or `spawning`, throws a clear error so the caller can
	 * retry once the agent reaches `idle`.
	 *
	 * Internal — does NOT enforce parent/child relationship. Use
	 * `routeFromCaller` from dispatcher-tool code paths.
	 */
	async route(id: string, message: string): Promise<void> {
		const runner = this.runners.get(id);
		if (!runner) {
			const row = await this.cfg.store.get(id);
			if (!row) throw new Error(`agent ${id} not found`);
			throw new Error(
				`agent ${id} is ${row.status} (not yet started or already terminal); cannot route`,
			);
		}
		if (runner.lifetime !== "persistent") {
			throw new Error(`agent ${id} is one-shot; cannot accept route`);
		}
		await runner.route(message);
	}

	/**
	 * Tree-edge-enforced route: the caller can only route to its own
	 * direct children. `callerKey` is the dispatcher's identity — the
	 * chat session key for the top-level coordinator, or the
	 * sub-coordinator's row id for nested dispatchers. Children of
	 * that dispatcher have `parent_session_key === callerKey`.
	 */
	async routeFromCaller(
		callerKey: string,
		targetId: string,
		message: string,
	): Promise<void> {
		const target = await this.cfg.store.get(targetId);
		if (!target) throw new Error(`agent ${targetId} not found`);
		if (target.parentSessionKey !== callerKey) {
			throw new Error(
				`agent ${targetId} is not a direct child of you (its parent is ${target.parentSessionKey}). ` +
					`You can only route to agents you spawned. To reach a deeper descendant, route through the intermediate coordinator.`,
			);
		}
		await this.route(targetId, message);
	}

	/**
	 * Upward edge: deliver a message from a spawned worker to its
	 * parent. The parent is whatever's named by `parent_session_key`:
	 *   - `"<channel>:<chatId>"` → top-level coordinator (AgentPool)
	 *   - any other id            → another row in this registry
	 */
	async reportToParent(callerId: string, message: string): Promise<void> {
		const caller = await this.cfg.store.get(callerId);
		if (!caller) throw new Error(`agent ${callerId} not found`);
		const parentKey = caller.parentSessionKey;
		// Heuristic: chat session keys are `channel:chatId` and contain
		// `:`. UUID worker ids do not.
		if (parentKey.includes(":")) {
			if (!this.cfg.deliverToTopLevel) {
				throw new Error(
					"top-level delivery not configured; report_to_parent unavailable for top-level children",
				);
			}
			await this.cfg.deliverToTopLevel(parentKey, `worker:${caller.name}`, message);
			return;
		}
		// Parent is another worker (a sub-coordinator).
		await this.route(parentKey, `[from child ${caller.name}] ${message}`);
	}

	async cancel(id: string): Promise<void> {
		const running = this.runners.get(id);
		if (running) {
			await running.cancel("cancelled by dispatcher");
			return;
		}
		const idx = this.queue.indexOf(id);
		if (idx !== -1) this.queue.splice(idx, 1);
		const row = await this.cfg.store.get(id);
		if (row && row.status !== "completed" && row.status !== "cancelled" && row.status !== "failed") {
			await this.cfg.store.setStatus(id, "cancelled", "cancelled by dispatcher");
		}
	}

	get(id: string): Promise<WorkerRow | null> {
		return this.cfg.store.get(id);
	}

	listForParent(parentSessionKey: string, limit = 20): Promise<WorkerRow[]> {
		return this.cfg.store.listByParent(parentSessionKey, limit);
	}

	/**
	 * Re-attach every non-terminal row from the previous process. Both
	 * `spawning`/`running` and `idle` rows are picked up — for `idle`
	 * the runner re-acquires the container and stays at idle (no auto
	 * prompt, just ready for the dispatcher's next route).
	 */
	async resumePending(): Promise<void> {
		const rows = await this.cfg.store.listByStatus(["spawning", "running", "idle"]);
		for (const row of rows) {
			if (!this.queue.includes(row.id)) this.queue.push(row.id);
		}
		if (rows.length > 0) {
			console.log(`[worker-registry] resuming ${rows.length} worker(s)`);
		}
		this.pump();
	}

	async shutdown(): Promise<void> {
		this.shuttingDown = true;
		this.queue.length = 0;
		await Promise.all(
			[...this.runners.values()].map((r) =>
				r.suspend().catch((err) =>
					console.error(`[worker-registry] suspend ${r.id}`, err),
				),
			),
		);
		this.runners.clear();
		await this.cfg.store.close();
	}

	// ── Internals ─────────────────────────────────────────────────────

	private pump(): void {
		while (
			!this.shuttingDown &&
			this.runners.size < this.cfg.maxConcurrent &&
			this.queue.length > 0
		) {
			const id = this.queue.shift()!;
			void this.spawnRunner(id);
		}
	}

	private async spawnRunner(id: string): Promise<void> {
		let row: WorkerRow | null = null;
		try {
			row = await this.cfg.store.get(id);
		} catch (err) {
			console.error(`[worker-registry] store.get(${id}) failed`, err);
			return;
		}
		if (!row) return;
		if (row.status === "completed" || row.status === "failed" || row.status === "cancelled") {
			return;
		}

		const resumed = row.messagesJson !== "[]" && row.messagesJson.length > 2;
		const toolsForKind =
			row.kind === "coordinator" ? this.cfg.coordinatorTools : this.cfg.executorTools;
		const runner = new WorkerRunner({
			row,
			store: this.cfg.store,
			sandbox: this.cfg.sandbox,
			model: this.cfg.model,
			channels: this.cfg.channels,
			toolsForKind,
			resumed,
			onTerminal: (tid) => {
				this.runners.delete(tid);
				this.pump();
			},
		});
		this.runners.set(id, runner);
		try {
			await runner.start();
		} catch (err) {
			console.error(`[worker-registry] runner ${id} start failed`, err);
			await this.cfg.store
				.markFailed(id, err instanceof Error ? err.message : String(err))
				.catch(() => undefined);
			await runner.dispose(true).catch(() => undefined);
			this.runners.delete(id);
			this.pump();
		}
	}
}
