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

import type { IncomingMessage, OutgoingReply } from "./channels/types.ts";
import type { MessageSender } from "./messaging/index.ts";
import { sessionKey } from "./channels/types.ts";
import { DEFAULT_DEBOUNCE, type DebounceConfig, SessionDebouncer } from "./debounce.ts";
import {
	type UserContent,
	buildUserContent,
	extractImagesFromContent,
	extractTextFromContent,
	imageContentToAttachment,
} from "./content.ts";
import { Tape, type TapeStore, entriesToLlmMessages } from "./tape/index.ts";

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
	sender: MessageSender;
	debounce?: DebounceConfig;
	/** Tape store for persistence. */
	tapeStore: TapeStore;
	/** Optional: resolve API keys from DB instead of env vars. */
	getApiKey?: (provider: string) => Promise<string | undefined> | string | undefined;
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

		// Persist user message to tape before handing to agent.
		const tape = new Tape({ store: this.config.tapeStore, name: key });
		try {
			await tape.append({
				kind: "message",
				payload: {
					role: "user",
					content: typeof content === "string" ? content : content.map((c) => ({ ...c })),
					timestamp: msg.timestamp,
				},
				meta: { channel: msg.channel, sender: msg.sender },
				date: new Date(msg.timestamp).toISOString(),
			});
		} catch (err) {
			log.error(`failed to persist user message to tape for ${key}`, err);
		}

		if (session.agent.state.isStreaming) {
			session.agent.steer({ role: "user", content, timestamp: msg.timestamp });
			return;
		}

		try {
			await session.agent.prompt({ role: "user", content, timestamp: msg.timestamp });
		} catch (err: any) {
			log.error(`prompt failed for session ${key}, sender "${msg.sender}":`, err);
			await this.config.sender.send(session.channelName, {
				chatId: session.lastIncoming.chatId,
				text: `Error: ${err?.message ?? String(err)}`,
			}).catch(() => undefined);
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

		// Create a Tape instance for this session.
		const tape = new Tape({ store: this.config.tapeStore, name: key });

		const agent = new Agent({
			initialState: {
				systemPrompt: this.config.systemPrompt,
				model: this.config.model,
				tools: allTools,
			},
			convertToLlm: (messages) =>
				messages.filter(
					(m) => m.role === "user" || m.role === "assistant" || m.role === "toolResult",
				),
			sessionId: key,
			getApiKey: this.config.getApiKey,
		});

		// Load history from tape and convert directly to LLM format.
		try {
			const entries = await tape.view().load();
			if (entries.length > 0) {
				const llmMessages = entriesToLlmMessages(entries);
				agent.state.messages = llmMessages as any;
				log.info(`restored ${llmMessages.length} messages from tape for session ${key}`);
			} else {
				await tape.anchor("session/start", {
					owner: "human",
					channel: msg.channel,
					chatId: msg.chatId,
				});
			}
		} catch (err) {
			log.error(`failed to load tape for session ${key}`, err);
		}

		const session: Session = {
			agent,
			channelName: msg.channel,
			lastIncoming: msg,
			dispose: perSession?.dispose,
		};

		// Per-message streaming state. Tracks how much of the current
		// assistant message has already been pushed as chunks, so we only
		// send the new tail and skip the final full reply when the channel
		// has been streaming.
		let streamMsgId: string | null = null;
		let streamSent = 0;
		let streamed = false;

		agent.subscribe(async (event) => {
			const channelName = session.channelName;
			const last = session.lastIncoming;
			const supportsStream = !!this.config.sender.sendChunk && !!this.config.sender.sendEnd;

			if (
				event.type === "message_start" &&
				event.message.role === "assistant"
			) {
				streamMsgId = `${last.chatId}-${Date.now()}-${Math.random()
					.toString(36)
					.slice(2, 8)}`;
				streamSent = 0;
				streamed = false;
				return;
			}

			if (
				supportsStream &&
				event.type === "message_update" &&
				event.message.role === "assistant"
			) {
				if (!streamMsgId) return;
				const text = extractText(event.message);
				if (text.length > streamSent) {
					const delta = text.slice(streamSent);
					streamSent = text.length;
					streamed = true;
					this.config.sender.sendChunk!(channelName, last.chatId, streamMsgId, delta);
				}
				return;
			}

			if (event.type === "message_end" && event.message.role === "assistant") {
				// Persist assistant message to tape.
				const am = event.message as Extract<AgentMessage, { role: "assistant" }>;
				try {
					await tape.append({
						kind: "message",
						payload: {
							role: "assistant",
							content: am.content.map((c: any) => ({ ...c })),
							api: (am as any).api ?? "",
							provider: (am as any).provider ?? "",
							model: (am as any).model ?? "",
							responseId: (am as any).responseId,
							usage: (am as any).usage,
							stopReason: (am as any).stopReason ?? "stop",
							errorMessage: (am as any).errorMessage,
							timestamp: (am as any).timestamp ?? Date.now(),
						},
						meta: {},
						date: new Date().toISOString(),
					});
				} catch (err) {
					log.error(`failed to persist assistant message to tape for ${key}`, err);
				}

				const text = extractText(event.message);

				// Flush any tail not yet streamed (final delta after last update).
				if (supportsStream && streamMsgId && text.length > streamSent) {
					const delta = text.slice(streamSent);
					streamSent = text.length;
					streamed = true;
					this.config.sender.sendChunk!(channelName, last.chatId, streamMsgId, delta);
				}

				if (supportsStream && streamed && streamMsgId) {
					this.config.sender.sendEnd!(channelName, last.chatId, streamMsgId);
					streamMsgId = null;
					return;
				}

				// Non-streaming path (or empty stream) → fall back to full reply.
				if (!text) return;
				const reply: OutgoingReply = {
					chatId: last.chatId,
					text,
					replyTo:
						typeof last.metadata["message_id"] === "string" || typeof last.metadata["message_id"] === "number"
							? String(last.metadata["message_id"])
							: undefined,
				};
				try {
					await this.config.sender.send(channelName, reply);
				} catch (err) {
					log.error(`failed to send reply via ${channelName} to chat ${last.chatId}`, err);
				}
				streamMsgId = null;
				return;
			}

			// Persist tool results to tape.
			if (event.type === "tool_execution_end") {
				try {
					await tape.append({
						kind: "tool_result",
						payload: {
							role: "toolResult",
							toolCallId: event.toolCallId,
							toolName: event.toolName,
							content: (event.result as any)?.content ?? [],
							details: (event.result as any)?.details,
							isError: event.isError,
							timestamp: Date.now(),
						},
						meta: {},
						date: new Date().toISOString(),
					});
				} catch (err) {
					log.error(`failed to persist tool result to tape for ${key}`, err);
				}
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
	return extractTextFromContent(msg.content as UserContent);
}
