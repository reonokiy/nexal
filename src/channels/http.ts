/**
 * HTTP channel — TS port of `crates/channel-http/src/lib.rs`.
 *
 * Endpoints:
 *   POST /send                 { chat_id?, sender?, text }  → enqueue IncomingMessage
 *   GET  /messages?chat_id=…   → drain outbox for that chat
 *   POST /response             { chat_id, text }            → skill-script callback
 *                                                            (see sandbox script reply path)
 *
 * The "response socket" here is folded into the same HTTP server on a
 * dedicated path rather than a separate Unix socket, because Bun's
 * `Bun.serve` does HTTP-over-TCP out of the box. Skill scripts inside
 * the sandbox can POST to the same `/response` URL over the network,
 * which is simpler than porting axum's Unix-socket dance.
 *
 * NOTE: this is the outer TCP surface. When the sandbox *proxies* a
 * call back to us, that routing lives in `exec-server` — out of scope
 * here.
 */
import type { Channel, IncomingMessage, OutgoingReply } from "./types.ts";
import type { CommandRegistry } from "../commands/registry.ts";
import { createLog } from "../log.ts";

const log = createLog("http");

type BunServer = ReturnType<typeof Bun.serve>;

export interface HttpChannelConfig {
	port: number;
	/** Listen address; defaults to 127.0.0.1. Use 0.0.0.0 only for tests. */
	host?: string;
	/** Shared command registry for slash commands. */
	commands?: CommandRegistry;
}

export class HttpChannel implements Channel {
	readonly name = "http";
	private server: BunServer | null = null;
	/** Per-chat outbox (FIFO). Replies accumulate here; GET /messages drains it. */
	private readonly outbox = new Map<string, string[]>();

	constructor(private readonly config: HttpChannelConfig) {}

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		const self = this;
		this.server = Bun.serve({
			port: this.config.port,
			hostname: this.config.host ?? "127.0.0.1",
			async fetch(req) {
				const url = new URL(req.url);

				if (req.method === "POST" && url.pathname === "/send") {
					const body = (await req.json()) as { chat_id?: string; sender?: string; text?: string };
					const chatId = body.chat_id ?? "default";
					const sender = body.sender ?? "http-user";
					const text = body.text ?? "";

					// Slash command interception.
					const cmds = self.config.commands;
					if (cmds && text.trim().startsWith("/")) {
						const parts = text.trim().slice(1).split(/\s+/);
						const name = parts[0]!;
						const args = parts.slice(1);
						if (cmds.has(name)) {
							const result = await cmds.execute(
								name,
								{ channel: "http", chatId, sender },
								args,
							);
							return Response.json({ ok: true, command: name, result: result?.text ?? null });
						}
					}

					const msg: IncomingMessage = {
						channel: "http",
						chatId,
						sender,
						text,
						timestamp: Date.now(),
						isMentioned: true,
						metadata: {},
						images: [],
					};
					onMessage(msg);
					return Response.json({ ok: true });
				}

				if (req.method === "GET" && url.pathname === "/messages") {
					const chatId = url.searchParams.get("chat_id") ?? "default";
					const messages = self.outbox.get(chatId) ?? [];
					self.outbox.delete(chatId);
					return Response.json({ messages });
				}

				if (req.method === "POST" && (url.pathname === "/response" || url.pathname === "/")) {
					// Sandbox skill-script → "the agent decided to reply" path.
					// Same effect as an outgoing reply from the agent.
					const body = (await req.json()) as { chat_id?: string; text?: string };
					if (typeof body.chat_id === "string" && typeof body.text === "string") {
						self.pushOutbox(body.chat_id, body.text);
					}
					return Response.json({ ok: true });
				}

				return new Response("not found", { status: 404 });
			},
		});
		log.info(`listening on http://${this.server.hostname}:${this.server.port}`);

		// Run forever (until stop()).
		return new Promise<void>((resolve) => {
			const check = setInterval(() => {
				if (!this.server) {
					clearInterval(check);
					resolve();
				}
			}, 1_000);
		});
	}

	async send(reply: OutgoingReply): Promise<void> {
		this.pushOutbox(reply.chatId, reply.text);
	}

	async stop(): Promise<void> {
		this.server?.stop();
		this.server = null;
	}

	private pushOutbox(chatId: string, text: string): void {
		const list = this.outbox.get(chatId) ?? [];
		list.push(text);
		this.outbox.set(chatId, list);
	}
}
