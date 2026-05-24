/**
 * WebSocket channel — chat protocol over @nexal/transport's RPC envelope.
 *
 * Transport:
 *   - Default: Unix domain socket at `~/.nexal/nexal.sock` (local dev)
 *   - Fallback: TCP on configurable host:port
 *
 * Wire protocol: msgpack-binary `{id,method,params}` /
 * `{id,result|error}` / `{method,params}` frames. Methods live in
 * `@nexal/transport`'s `chat.ts` (`chat/authenticate`, `chat/send`,
 * `chat/reply`, …).
 *
 * The `fetch` handler also accepts `POST /send` for curl debugging;
 * it skips the chat protocol entirely and synthesizes an IncomingMessage.
 */
import { mkdirSync, unlinkSync } from "node:fs";
import { dirname } from "node:path";

import { createLog } from "../log.ts";
import { verifySupabaseJwt, isAuthEnabled } from "../auth.ts";
import type {
	Channel,
	IncomingMessage,
	OutgoingReply,
	TypingHandle,
} from "./types.ts";
import { waitUntilStopped } from "./types.ts";
import type { CommandRegistry } from "../commands/registry.ts";
import { registerChannel } from "./factory.ts";
import {
	createAcceptedWebSocketConnection,
	handleChatNotifications,
	handleChatRequests,
	type AcceptedWebSocketConnection,
	type ChatCommandResultParams,
	type ChatReplyChunkParams,
	type ChatReplyEndParams,
	type ChatReplyParams,
	type ChatTypingParams,
	type ImageBlock,
	type WebSocketPeer,
} from "@nexal/transport";
import type { Connection } from "@nexal/transport";

const log = createLog("ws");

const DEFAULT_CHAT_ID = "default";
const DEFAULT_SENDER = "ws-user";

type BunServer = ReturnType<typeof Bun.serve>;
type BunWs = import("bun").ServerWebSocket<WsData>;

export interface WsChannelConfig {
	/** Unix socket path. Takes precedence over port. */
	unix?: string;
	/** TCP port (only used when unix is unset). */
	port?: number;
	/** TCP bind address (default 127.0.0.1). */
	host?: string;
	/** Shared command registry for slash commands. */
	commands?: CommandRegistry;
}

interface WsState {
	chatId: string;
	authed: boolean;
	userId?: string;
}

interface WsData {
	state: WsState;
	wired: AcceptedWebSocketConnection | null;
}

export class WsChannel implements Channel {
	readonly name = "ws";
	private server: BunServer | null = null;
	private readonly clients = new Map<string, Set<BunWs>>();
	private onMessage: ((msg: IncomingMessage) => void) | null = null;

	constructor(private readonly config: WsChannelConfig) {}

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		this.onMessage = onMessage;
		const self = this;

		// Clean up stale Unix socket if needed.
		if (this.config.unix) {
			mkdirSync(dirname(this.config.unix), { recursive: true });
			try {
				unlinkSync(this.config.unix);
			} catch {
				// No stale socket — fine.
			}
		}

		const serveOpts: Parameters<typeof Bun.serve>[0] = {
			...(this.config.unix
				? { unix: this.config.unix }
				: {
						port: this.config.port ?? 3001,
						hostname: this.config.host ?? "127.0.0.1",
					}),

			fetch(req, server) {
				const url = new URL(req.url);

				// Health check for Fly.io probes.
				if (req.method === "GET" && url.pathname === "/health") {
					return new Response("ok", { status: 200 });
				}

				// WebSocket upgrade — any GET request.
				if (req.method === "GET" && req.headers.get("upgrade") === "websocket") {
					const data: WsData = {
						state: {
							chatId: DEFAULT_CHAT_ID,
							// If auth is disabled globally, treat the socket as pre-authed
							// so notifications can flow without an authenticate roundtrip.
							authed: !isAuthEnabled(),
						},
						wired: null,
					};
					if (server.upgrade(req, { data })) {
						return undefined as unknown as Response;
					}
					return new Response("WebSocket upgrade failed", { status: 500 });
				}

				// POST /send — curl-compatible fallback, bypasses the chat protocol.
				if (req.method === "POST" && url.pathname === "/send") {
					return (async () => {
						const body = (await req.json()) as {
							chat_id?: string;
							sender?: string;
							text?: string;
						};
						self.fireIncoming(
							body.chat_id ?? DEFAULT_CHAT_ID,
							body.sender ?? DEFAULT_SENDER,
							body.text ?? "",
						);
						return Response.json({ ok: true });
					})();
				}

				return new Response("not found", { status: 404 });
			},

			websocket: {
				open(ws: BunWs) {
					self.attach(ws);
				},

				message(ws: BunWs, raw: string | Buffer) {
					const wired = ws.data.wired;
					if (!wired) return;
					wired.transport.receive(raw);
				},

				close(ws: BunWs) {
					ws.data.wired?.transport.disconnect();
					self.removeClient(ws.data.state.chatId, ws);
				},
			},
		};

		this.server = Bun.serve(serveOpts);
		const addr = this.config.unix
			? this.config.unix
			: `${this.server.hostname}:${this.server.port}`;
		log.info(`listening on ${addr}`);

		// Block until stop() is called.
		return waitUntilStopped(() => !this.server);
	}

	async send(reply: OutgoingReply): Promise<void> {
		this.broadcast(reply.chatId, "chat/reply", {
			chatId: reply.chatId,
			text: reply.text,
			...(reply.metadata
				? { metadata: reply.metadata as ChatReplyParams["metadata"] }
				: {}),
		});
	}

	sendChunk(chatId: string, messageId: string, delta: string): void {
		this.broadcast(chatId, "chat/replyChunk", {
			chatId,
			messageId,
			delta,
		} satisfies ChatReplyChunkParams);
	}

	sendEnd(chatId: string, messageId: string): void {
		this.broadcast(chatId, "chat/replyEnd", {
			chatId,
			messageId,
		} satisfies ChatReplyEndParams);
	}

	startTyping(chatId: string): TypingHandle | null {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return null;
		const params: ChatTypingParams = { chatId };
		const send = () => this.broadcast(chatId, "chat/typing", params);
		send();
		const timer = setInterval(send, 4_000);
		return { stop: () => clearInterval(timer) };
	}

	async stop(): Promise<void> {
		for (const set of this.clients.values()) {
			for (const ws of set) {
				ws.data.wired?.transport.disconnect();
				ws.close(1000, "shutdown");
			}
		}
		this.clients.clear();
		this.server?.stop();
		this.server = null;
	}

	// ── Internals ───────────────────────────────────────────────────

	private attach(ws: BunWs): void {
		const state = ws.data.state;
		const peer: WebSocketPeer = {
			get readyState() {
				return ws.readyState;
			},
			send: (data) => ws.send(data),
			close: () => ws.close(),
		};

		const wired = createAcceptedWebSocketConnection(peer);
		ws.data.wired = wired;
		this.addClient(state.chatId, ws);

		const conn = wired.connection;

		handleChatRequests(conn, {
			authenticate: async ({ token }) => {
				if (!token) throw new Error("missing token");
				const user = await verifySupabaseJwt(token);
				if (!user) throw new Error("invalid or expired token");
				state.authed = true;
				state.userId = user.sub;
				return {
					userId: user.sub,
					...(user.email ? { email: user.email } : {}),
				};
			},
			listCommands: () => {
				if (!state.authed) throw new Error("not authenticated");
				const commands = this.config.commands
					? this.config.commands
							.list()
							.map((c) => ({ name: c.name, description: c.description }))
					: [];
				return { commands };
			},
		});

		handleChatNotifications(conn, {
			send: ({ chatId, sender, text, images }) => {
				if (!state.authed) return;
				const target = chatId ?? DEFAULT_CHAT_ID;
				this.rebindChat(ws, target);
				this.fireIncoming(target, sender ?? DEFAULT_SENDER, text ?? "", images);
			},
			command: ({ chatId, sender, name, args }) => {
				if (!state.authed) return;
				const target = chatId ?? DEFAULT_CHAT_ID;
				this.rebindChat(ws, target);
				this.handleCommand(conn, target, sender ?? DEFAULT_SENDER, name, args ?? []);
			},
		});
	}

	private rebindChat(ws: BunWs, chatId: string): void {
		if (chatId === ws.data.state.chatId) return;
		this.removeClient(ws.data.state.chatId, ws);
		ws.data.state.chatId = chatId;
		this.addClient(chatId, ws);
	}

	private addClient(chatId: string, ws: BunWs): void {
		let set = this.clients.get(chatId);
		if (!set) {
			set = new Set();
			this.clients.set(chatId, set);
		}
		set.add(ws);
	}

	private removeClient(chatId: string, ws: BunWs): void {
		const set = this.clients.get(chatId);
		if (!set) return;
		set.delete(ws);
		if (set.size === 0) this.clients.delete(chatId);
	}

	private broadcast(chatId: string, method: string, params: unknown): void {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return;
		for (const ws of set) {
			ws.data.wired?.connection.notify(method, params);
		}
	}

	private handleCommand(
		conn: Connection,
		chatId: string,
		sender: string,
		name: string,
		args: string[],
	): void {
		const cmds = this.config.commands;
		if (!cmds || !cmds.has(name)) {
			conn.notify("chat/commandResult", {
				chatId,
				name,
				error: `unknown command: /${name}`,
			} satisfies ChatCommandResultParams);
			return;
		}
		void cmds
			.execute(name, { channel: "ws", chatId, sender }, args)
			.then((result) => {
				conn.notify("chat/commandResult", {
					chatId,
					name,
					text: result?.text ?? "",
					...(result && "data" in result ? { data: result.data } : {}),
				} satisfies ChatCommandResultParams);
			})
			.catch((err) => {
				log.error(`command /${name} failed`, err);
				conn.notify("chat/commandResult", {
					chatId,
					name,
					error: err instanceof Error ? err.message : String(err),
				} satisfies ChatCommandResultParams);
			});
	}

	private fireIncoming(
		chatId: string,
		sender: string,
		text: string,
		images?: ImageBlock[],
	): void {
		this.onMessage?.({
			channel: "ws",
			chatId,
			sender,
			text,
			timestamp: Date.now(),
			isMentioned: true,
			metadata: {},
			images:
				images?.map((img) => ({
					data: img.data,
					mimeType: img.mimeType,
					filename: "clipboard.png",
				})) ?? [],
		});
	}
}

registerChannel("ws", ({ cfg, commands }) => {
	return new WsChannel({
		port: Number(cfg.port ?? 3000),
		host: (cfg.host as string | undefined) ?? "0.0.0.0",
		unix: cfg.unix as string | undefined,
		commands,
	});
});
