/**
 * AgentPool — one `pi-agent-core` Agent per (channel, chatId), with a
 * per-session debouncer sitting in front.
 *
 * Ports the semantics of `crates/agent/src/pool.rs` + the
 * `SessionRunner` debouncer in `crates/channel-core/src/debounce.rs`:
 *
 *  - Incoming messages go to `SessionRunner.process`; the debouncer
 *    batches follow-ups before handing a single merged
 *    `IncomingMessage` to `handleMerged`.
 *  - `handleMerged` fetches (or lazily constructs) the chat's Agent
 *    and drives one turn via `agent.prompt`. If the Agent is already
 *    mid-turn, the merged message is instead injected via
 *    `agent.steer` so the running loop sees it on its next LLM hop.
 *  - Assistant `message_end` events are converted back into
 *    `OutgoingReply`s and dispatched through the source channel.
 */
import { Agent, type AgentMessage, type AgentTool } from "@mariozechner/pi-agent-core";
import type { Model } from "@mariozechner/pi-ai";

import type { Channel, IncomingMessage, OutgoingReply } from "./channels/types.ts";
import { sessionKey } from "./channels/types.ts";
import { DEFAULT_DEBOUNCE, type DebounceConfig, SessionRunner } from "./channels/debounce.ts";

export interface AgentPoolConfig {
	systemPrompt: string;
	model: Model<any>;
	/** Shared tools available to every session. */
	tools: AgentTool<any>[];
	/**
	 * Optional per-session tool factory. Called once when a session is
	 * first created; returned tools are appended to `tools`. The returned
	 * `dispose` is invoked when the session is shut down — use it to
	 * clean up per-session resources (e.g. nexal-agent subprocess).
	 */
	toolsFor?: (sessionKey: string) => Promise<{
		tools: AgentTool<any>[];
		dispose?: () => Promise<void>;
	}>;
	channels: Map<string, Channel>;
	debounce?: DebounceConfig;
}

interface Session {
	agent: Agent;
	channelName: string;
	lastIncoming: IncomingMessage;
	dispose?: () => Promise<void>;
}

export class AgentPool {
	private readonly sessions = new Map<string, Session>();
	private readonly pending = new Map<string, Promise<Session>>();
	private readonly runners = new Map<string, SessionRunner>();

	constructor(private readonly config: AgentPoolConfig) {}

	/** Entry from channels: hand a message to the per-session debouncer. */
	handle(msg: IncomingMessage): void {
		const key = sessionKey(msg);
		let runner = this.runners.get(key);
		if (!runner) {
			runner = new SessionRunner(key, this.config.debounce ?? DEFAULT_DEBOUNCE, (m) =>
				this.handleMerged(m),
			);
			this.runners.set(key, runner);
		}
		runner.process(msg);
	}

	/**
	 * Inject a synthetic message into a chat session's debouncer — used
	 * when a spawned worker's `report_to_parent` lands on the top-level
	 * coordinator (which has no DB row, only an in-memory Agent here).
	 *
	 * `sessionKey` is `"<channel>:<chatId>"`; the synthesized
	 * IncomingMessage carries that channel/chatId so the dispatcher's
	 * eventual reply still flows back to the correct chat.
	 */
	injectMessage(sessionKeyStr: string, sender: string, text: string): void {
		const sepIdx = sessionKeyStr.indexOf(":");
		if (sepIdx === -1) {
			console.error(`[agent-pool] injectMessage: malformed sessionKey ${sessionKeyStr}`);
			return;
		}
		const channel = sessionKeyStr.slice(0, sepIdx);
		const chatId = sessionKeyStr.slice(sepIdx + 1);
		this.handle({
			channel,
			chatId,
			sender,
			text,
			timestamp: Date.now(),
			isMentioned: true,
			metadata: {},
			images: [],
		});
	}

	/** Called by the debouncer with the merged batch. */
	private async handleMerged(msg: IncomingMessage): Promise<void> {
		const key = sessionKey(msg);
		const session = await this.getOrCreate(key, msg);
		session.lastIncoming = msg;

		if (session.agent.state.isStreaming) {
			session.agent.steer({
				role: "user",
				content: msg.text,
				timestamp: msg.timestamp,
			});
			return;
		}

		await session.agent.prompt(msg.text);
	}

	private async getOrCreate(key: string, msg: IncomingMessage): Promise<Session> {
		const existing = this.sessions.get(key);
		if (existing) return existing;

		const inflight = this.pending.get(key);
		if (inflight) return inflight;

		const created = this.createSession(key, msg).finally(() => {
			this.pending.delete(key);
		});
		this.pending.set(key, created);
		const session = await created;
		this.sessions.set(key, session);
		return session;
	}

	private async createSession(key: string, msg: IncomingMessage): Promise<Session> {
		const perSession = this.config.toolsFor ? await this.config.toolsFor(key) : undefined;
		const allTools = [...this.config.tools, ...(perSession?.tools ?? [])];

		const agent = new Agent({
			initialState: {
				systemPrompt: this.config.systemPrompt,
				model: this.config.model,
				tools: allTools,
			},
			convertToLlm: (messages: AgentMessage[]) =>
				messages.filter(
					(m) => m.role === "user" || m.role === "assistant" || m.role === "toolResult",
				),
			sessionId: key,
		});

		const session: Session = {
			agent,
			channelName: msg.channel,
			lastIncoming: msg,
			dispose: perSession?.dispose,
		};

		agent.subscribe(async (event) => {
			if (event.type !== "message_end" || event.message.role !== "assistant") return;
			const text = extractText(event.message);
			if (!text) return;
			const channel = this.config.channels.get(session.channelName);
			if (!channel) return;
			const last = session.lastIncoming;
			const reply: OutgoingReply = {
				chatId: last.chatId,
				text,
				replyTo:
					typeof last.metadata["message_id"] === "string" || typeof last.metadata["message_id"] === "number"
						? String(last.metadata["message_id"])
						: undefined,
			};
			try {
				await channel.send(reply);
			} catch (err) {
				console.error(`[agent-pool] send failed for ${session.channelName}:${last.chatId}`, err);
			}
		});

		return session;
	}

	async shutdown(): Promise<void> {
		for (const s of this.sessions.values()) s.agent.abort();
		await Promise.all(
			[...this.sessions.values()].map((s) => s.agent.waitForIdle().catch(() => undefined)),
		);
		await Promise.all([...this.runners.values()].map((r) => r.shutdown()));
		await Promise.all(
			[...this.sessions.values()].map((s) => s.dispose?.().catch(() => undefined)),
		);
		this.sessions.clear();
		this.runners.clear();
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
