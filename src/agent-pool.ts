/**
 * AgentPool — one `pi-agent-core` Agent per (channel, chatId), with a
 * per-session debouncer sitting in front.
 *
 * Ports the semantics of `crates/agent/src/pool.rs` + the
 * `SessionDebouncer` debouncer in `crates/channel-core/src/debounce.rs`:
 *
 *  - Incoming messages go to `SessionDebouncer.process`; the debouncer
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
import { createLog } from "./log.ts";

const log = createLog("pool");
import type { Model } from "@mariozechner/pi-ai";

import type { Channel, IncomingMessage, OutgoingReply } from "./channels/types.ts";
import { sessionKey } from "./channels/types.ts";
import { DEFAULT_DEBOUNCE, type DebounceConfig, SessionDebouncer } from "./channels/debounce.ts";
import {
	type UserContent,
	buildUserContent,
	extractImagesFromContent,
	extractTextFromContent,
	imageContentToAttachment,
} from "./content.ts";

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
	private readonly debouncers = new Map<string, SessionDebouncer>();

	constructor(private readonly config: AgentPoolConfig) {}

	/** Entry from channels: hand a message to the per-session debouncer. */
	handle(msg: IncomingMessage): void {
		const key = sessionKey(msg);
		let debouncer = this.debouncers.get(key);
		if (!debouncer) {
			debouncer = new SessionDebouncer(key, this.config.debounce ?? DEFAULT_DEBOUNCE, (m) =>
				this.handleMerged(m),
			);
			this.debouncers.set(key, debouncer);
		}
		debouncer.process(msg);
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
	forwardChildReport(sessionKeyStr: string, sender: string, content: UserContent): void {
		const sepIdx = sessionKeyStr.indexOf(":");
		if (sepIdx === -1) {
			log.error(`malformed session key "${sessionKeyStr}", expected "channel:chatId" format`);
			return;
		}
		const channel = sessionKeyStr.slice(0, sepIdx);
		const chatId = sessionKeyStr.slice(sepIdx + 1);
		this.handle({
			channel,
			chatId,
			sender,
			text: extractTextFromContent(content),
			timestamp: Date.now(),
			isMentioned: true,
			metadata: {},
			images: extractImagesFromContent(content).map(imageContentToAttachment),
		});
	}

	/** Called by the debouncer with the merged batch. */
	private async handleMerged(msg: IncomingMessage): Promise<void> {
		const key = sessionKey(msg);
		const session = await this.getOrCreate(key, msg);
		session.lastIncoming = msg;

		const content = buildUserContent(msg.text, msg.images);

		if (session.agent.state.isStreaming) {
			session.agent.steer({ role: "user", content, timestamp: msg.timestamp });
			return;
		}

		try {
			await session.agent.prompt({ role: "user", content, timestamp: msg.timestamp });
		} catch (err: any) {
			log.error(`prompt failed for session ${key}, sender "${msg.sender}":`, err);
			const channel = this.config.channels.get(session.channelName);
			if (channel) {
				await channel.send({
					chatId: session.lastIncoming.chatId,
					text: `Error: ${err?.message ?? String(err)}`,
				}).catch(() => undefined);
			}
		}
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
				log.error(`failed to send reply via ${session.channelName} to chat ${last.chatId}`, err);
			}
		});

		return session;
	}

	async shutdown(): Promise<void> {
		for (const s of this.sessions.values()) s.agent.abort();
		await Promise.all(
			[...this.sessions.values()].map((s) => s.agent.waitForIdle().catch(() => undefined)),
		);
		await Promise.all([...this.debouncers.values()].map((r) => r.shutdown()));
		await Promise.all(
			[...this.sessions.values()].map((s) => s.dispose?.().catch(() => undefined)),
		);
		this.sessions.clear();
		this.debouncers.clear();
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
