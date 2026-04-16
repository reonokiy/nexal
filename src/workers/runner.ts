/**
 * WorkerRunner — one sub-agent instance (coordinator or executor).
 *
 * Owns:
 *   - a Podman container (via `GatewayClient.acquireAgent("worker:<id>")`)
 *   - an `AgentClient` (gateway-mediated) for that container
 *   - one `Agent` (from pi-agent-core)
 *
 * Tool set is selected by `kind`:
 *   - `"coordinator"` — dispatcher tools only (spawn_…, route_to_agent, …),
 *                       NO bash. Lets sub-coordinators recursively spawn
 *                       their own children.
 *   - `"executor"`    — `bash` + `send_update`. Does the work.
 *
 * Termination is selected by `lifetime`:
 *   - `"persistent"` — on `agent_end`, flip to `idle` and stay alive.
 *                      `route(message)` feeds the next instruction (via
 *                      `Agent.steer` if streaming, else `Agent.prompt`).
 *                      Coordinators are always persistent.
 *   - `"shot"`       — on `agent_end`, mark `completed`, release the
 *                      sandbox, notify the registry. Only valid for
 *                      executors.
 *
 * All:
 *   - persist `messages_json` + `turn_count` on `turn_end` (debounced)
 *   - flush synchronously on terminal transitions
 *   - surface `errorMessage` to the chat as `❌ failed: …`
 *   - send_policy `"final"`/`"all"` is honored for executors. Coord
 *     output is dispatching prose — usually noisy — so coordinators
 *     ignore policy and only emit via their tools' explicit messages
 *     (in practice, dispatcher tools don't talk to the chat directly).
 */
import { Agent, type AgentMessage, type AgentTool } from "@mariozechner/pi-agent-core";
import { createLog } from "../log.ts";
import type { Model } from "@mariozechner/pi-ai";

import type { Channel } from "../channels/types.ts";
import type { ProxySpec } from "../config.ts";
import type { GatewayClient } from "../gateway/client.ts";
import type { AgentClient } from "../gateway/agent_client.ts";
import { createBashTool } from "../tools/bash.ts";
import { deserializeMessages, serializeMessages } from "./serialize.ts";
import type { SendPolicy, WorkerKind, WorkerLifetime, WorkerRow, WorkerStore } from "./store.ts";

const PERSIST_DEBOUNCE_MS = 250;

const RESUME_NUDGE = [
	"[nexal] You were interrupted by a process restart.",
	"Your shell container has been re-attached; filesystem side-effects in /workspace",
	"persist, but anything done outside /workspace or in-memory container state is gone.",
	"Inspect /workspace, figure out where you left off, continue the work,",
	"and call send_update when you have progress to share.",
].join(" ");

export interface WorkerRunnerDeps {
	row: WorkerRow;
	store: WorkerStore;
	gateway: GatewayClient;
	model: Model<any>;
	channels: Map<string, Channel>;
	/**
	 * Tool factory called once when the agent is constructed. The
	 * registry routes here based on `runner.row.kind`:
	 *   - executor    → `[bash, send_update, …]`
	 *   - coordinator → `[spawn_executor, spawn_coordinator, …]` (no bash)
	 */
	toolsForKind: (runner: WorkerRunner) => AgentTool<any>[];
	resumed: boolean;
	/** Proxies to register for executors on spawn. */
	executorProxies?: ProxySpec[];
	/** Called once a shot executor reaches a terminal state. */
	onTerminal: (id: string) => void;
}

export class WorkerRunner {
	readonly id: string;
	readonly kind: WorkerKind;
	readonly lifetime: WorkerLifetime;
	readonly row: WorkerRow;
	readonly sandboxKey: string;
	private readonly log;
	private agent?: Agent;
	private client?: AgentClient;
	private disposed = false;
	private persistTimer: ReturnType<typeof setTimeout> | null = null;
	private latestTurnCount: number;

	constructor(private readonly deps: WorkerRunnerDeps) {
		this.id = deps.row.id;
		this.kind = deps.row.kind;
		this.lifetime = deps.row.lifetime;
		this.row = deps.row;
		this.sandboxKey = `worker:${deps.row.id}`;
		this.latestTurnCount = deps.row.turnCount;
		this.log = createLog(`worker:${this.id}`);
	}

	/**
	 * Acquire sandbox + Agent and run the initial prompt (or the
	 * resume nudge when re-attaching after a restart). For persistent
	 * workers `start()` resolves once the initial run reaches idle;
	 * for shot workers it resolves once the run terminates.
	 */
	async start(): Promise<void> {
		const { row, gateway, model, store } = this.deps;

		// Coordinators don't need a container at all (they only call
		// dispatcher tools that talk to the registry). Skipping the
		// container for coordinators saves real resources, especially
		// once sub-coordinators are common.
		if (this.kind === "executor") {
			this.client = await gateway.acquireAgent(this.sandboxKey);
			await this.setupProxies();
		}

		const initialMessages = deserializeMessages(row.messagesJson);
		const tools = this.deps.toolsForKind(this);
		const agent = new Agent({
			initialState: {
				systemPrompt: row.systemPrompt,
				model,
				tools,
				messages: initialMessages,
			},
			convertToLlm: (messages) =>
				messages.filter(
					(m) => m.role === "user" || m.role === "assistant" || m.role === "toolResult",
				),
			sessionId: this.sandboxKey,
		});
		this.agent = agent;

		this.wireEvents(agent);

		await store.markStarted(this.id);

		if (this.deps.resumed && initialMessages.length > 0) {
			await agent.prompt(RESUME_NUDGE);
		} else if (row.initialPrompt) {
			await agent.prompt(row.initialPrompt);
		} else {
			// Persistent agent spawned without an initial prompt — flip
			// to idle immediately so the parent can route to it.
			await store.markIdle(this.id, serializeMessages(initialMessages));
		}
	}

	/** Exposed so the toolsForKind factory can attach bash for executors. */
	get execClient(): AgentClient | undefined {
		return this.client;
	}

	/**
	 * Coordinator → existing persistent agent (coordinator or
	 * executor). If mid-run, queue via `agent.steer`; else
	 * `agent.prompt`. Throws for shot lifetime.
	 */
	async route(message: string): Promise<void> {
		if (this.lifetime !== "persistent") {
			throw new Error(`worker ${this.id} is one-shot; cannot accept route`);
		}
		const agent = this.agent;
		if (!agent) throw new Error(`worker ${this.id} not started`);
		const msg: AgentMessage = { role: "user", content: message, timestamp: Date.now() };
		if (agent.state.isStreaming) {
			agent.steer(msg);
			return;
		}
		await this.deps.store.markStarted(this.id);
		await agent.prompt(msg);
	}

	get currentAgent(): Agent | undefined {
		return this.agent;
	}

	async cancel(reason = "cancelled"): Promise<void> {
		if (this.disposed) return;
		this.agent?.abort();
		await this.agent?.waitForIdle().catch(() => undefined);
		await this.flushNow();
		await this.deps.store.setStatus(this.id, "cancelled", reason);
		await this.dispose(true);
		this.deps.onTerminal(this.id);
	}

	/**
	 * Process-shutdown teardown: abort the agent, flush, detach (NOT
	 * release) the sandbox so the container survives. The DB row is
	 * left in whatever non-terminal state it was in (`running` or
	 * `idle`) so the next process re-picks it up.
	 */
	async suspend(): Promise<void> {
		if (this.disposed) return;
		this.agent?.abort();
		await this.agent?.waitForIdle().catch(() => undefined);
		await this.flushNow();
		await this.dispose(false);
	}

	async dispose(releaseContainer: boolean): Promise<void> {
		if (this.disposed) return;
		this.disposed = true;
		if (this.persistTimer) {
			clearTimeout(this.persistTimer);
			this.persistTimer = null;
		}
		try {
			await this.client?.close();
		} catch {}
		if (releaseContainer) {
			await this.deps.gateway.releaseAgent(this.sandboxKey).catch(() => undefined);
		} else {
			await this.deps.gateway.detachAgent(this.sandboxKey).catch(() => undefined);
		}
	}

	async sendToSourceChat(text: string, opts?: { replyTo?: string }): Promise<void> {
		if (!text.trim()) return;
		const ch = this.deps.channels.get(this.deps.row.sourceChannel);
		if (!ch) {
			this.log.error(
				`source channel "${this.deps.row.sourceChannel}" is not registered, cannot send message`,
			);
			return;
		}
		try {
			await ch.send({
				chatId: this.deps.row.sourceChatId,
				text: `[${this.deps.row.name}] ${text}`,
				replyTo: opts?.replyTo ?? this.deps.row.sourceReplyTo ?? undefined,
			});
		} catch (err) {
			this.log.error(`failed to send message via ${this.deps.row.sourceChannel} to chat ${this.deps.row.sourceChatId}`, err);
		}
	}

	// ── Internals ─────────────────────────────────────────────────────

	/**
	 * Register every configured proxy with the gateway. Gateway in turn
	 * tells nexal-agent (in this container) to bring up a unix socket
	 * under `/run/nexal/proxy/<name>.socket` that forwards to
	 * the gateway, which adds auth headers and proxies to the real
	 * upstream. The executor uses the socket directly:
	 *   `curl --unix-socket /run/nexal/proxy/jina.socket http://x/v1/search`
	 *
	 * No-op when there's no gateway, no proxies configured, or the
	 * client has no `agentId`.
	 */
	private async setupProxies(): Promise<void> {
		const client = this.client;
		const proxies = this.deps.executorProxies ?? [];
		if (!client || proxies.length === 0) return;
		const agentId = client.agentId;
		if (!agentId) return;

		// Make sure the directory exists. Sockets are created by
		// nexal-agent during gateway/register_proxy itself; we just
		// need the parent dir present.
		await client
			.runCommand(["/bin/sh", "-c", "mkdir -p /run/nexal/proxy"], {
				timeoutMs: 5_000,
			})
			.catch((err) => this.log.error("failed to create proxy socket directory in container", err));

		for (const spec of proxies) {
			try {
				await this.deps.gateway.invoke("gateway/register_proxy", {
					agent_id: agentId,
					name: spec.name,
					upstream_url: spec.upstreamUrl,
					headers: spec.headers ?? {},
				});
			} catch (err) {
				this.log.error(
					`failed to register proxy "${spec.name}" -> ${spec.upstreamUrl}`,
					err,
				);
			}
		}
	}

	private wireEvents(agent: Agent): void {
		agent.subscribe(async (event) => {
			try {
				if (event.type === "turn_end") {
					this.latestTurnCount += 1;
					this.scheduleFlush();
					return;
				}
				if (event.type === "message_end" && event.message.role === "assistant") {
					if (this.deps.row.sendPolicy === "all") {
						const text = extractText(event.message);
						if (text) await this.sendToSourceChat(text);
					}
					return;
				}
				if (event.type === "agent_end") {
					await this.handleAgentEnd(agent, event.messages);
				}
			} catch (err) {
				this.log.error(`error handling "${event.type}" event`, err);
			}
		});
	}

	private async handleAgentEnd(agent: Agent, messages: AgentMessage[]): Promise<void> {
		if (this.disposed) return;
		const errorMessage = agent.state.errorMessage;
		const policy = this.deps.row.sendPolicy as SendPolicy;
		await this.flushNow(messages);

		if (errorMessage) {
			await this.deps.store.markFailed(this.id, errorMessage);
			await this.sendToSourceChat(`❌ failed: ${errorMessage}`);
			await this.dispose(true);
			this.deps.onTerminal(this.id);
			return;
		}

		// Coordinators talk to the user via their dispatcher tool calls,
		// not via assistant content — so suppress send_policy for them
		// (their assistant text is dispatching prose, usually noise).
		if (this.kind === "executor" && (policy === "final" || policy === "all")) {
			const final = extractLastAssistantText(messages);
			if (final) await this.sendToSourceChat(final);
		}

		if (this.lifetime === "shot") {
			await this.deps.store.markCompleted(this.id, serializeMessages(messages));
			await this.dispose(true);
			this.deps.onTerminal(this.id);
			return;
		}

		// Persistent: stay alive, accept future routes.
		await this.deps.store.markIdle(this.id, serializeMessages(messages));
	}

	private scheduleFlush(): void {
		if (this.persistTimer) return;
		this.persistTimer = setTimeout(() => {
			this.persistTimer = null;
			void this.flushNow();
		}, PERSIST_DEBOUNCE_MS);
	}

	private async flushNow(messagesOverride?: AgentMessage[]): Promise<void> {
		if (this.persistTimer) {
			clearTimeout(this.persistTimer);
			this.persistTimer = null;
		}
		const messages = messagesOverride ?? this.agent?.state.messages;
		if (!messages) return;
		try {
			await this.deps.store.setMessages(
				this.id,
				serializeMessages(messages),
				this.latestTurnCount,
			);
		} catch (err) {
			this.log.error(`failed to persist messages after turn ${this.latestTurnCount}`, err);
		}
	}
}

function extractText(msg: AgentMessage): string {
	const content = (msg as { content?: unknown }).content;
	if (typeof content === "string") return content;
	if (!Array.isArray(content)) return "";
	const parts: string[] = [];
	for (const block of content) {
		if (typeof block === "string") parts.push(block);
		else if (block && typeof block === "object" && "type" in block && (block as any).type === "text") {
			parts.push(String((block as any).text ?? ""));
		}
	}
	return parts.join("");
}

function extractLastAssistantText(messages: AgentMessage[]): string {
	for (let i = messages.length - 1; i >= 0; i--) {
		const m = messages[i]!;
		if (m.role === "assistant") return extractText(m);
	}
	return "";
}
