/**
 * WebSocket channel — push-based replacement for the polling HTTP channel.
 *
 * Transport:
 *   - Default: Unix domain socket at `~/.nexal/nexal.sock` (local dev)
 *   - Fallback: TCP on configurable host:port
 *
 * Wire protocol (JSON text frames):
 *   Client → Server:  { type: "send", chat_id?, sender?, text }
 *   Server → Client:  { type: "reply", chat_id, text }
 *   Server → Client:  { type: "typing", chat_id }
 *
 * The `fetch` handler also accepts `POST /send` for curl debugging,
 * same schema as the client→server WS frame.
 */
import { mkdirSync, unlinkSync } from "node:fs";
import { dirname } from "node:path";

import { createLog } from "../log.ts";
import type {
	Channel,
	IncomingMessage,
	OutgoingReply,
	TypingHandle,
} from "./types.ts";

const log = createLog("ws");

type BunServer = ReturnType<typeof Bun.serve>;

export interface WsChannelConfig {
	/** Unix socket path. Takes precedence over port. */
	unix?: string;
	/** TCP port (only used when unix is unset). */
	port?: number;
	/** TCP bind address (default 127.0.0.1). */
	host?: string;
}

interface WsData {
	chatId: string;
}

export class WsChannel implements Channel {
	readonly name = "ws";
	private server: BunServer | null = null;
	private readonly clients = new Map<
		string,
		Set<import("bun").ServerWebSocket<WsData>>
	>();
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

				// WebSocket upgrade — any GET request.
				if (req.method === "GET" && req.headers.get("upgrade") === "websocket") {
					if (server.upgrade(req, { data: { chatId: "default" } as WsData })) {
						return undefined as unknown as Response;
					}
					return new Response("WebSocket upgrade failed", { status: 500 });
				}

				// POST /send — curl-compatible fallback.
				if (req.method === "POST" && url.pathname === "/send") {
					return (async () => {
						const body = (await req.json()) as {
							chat_id?: string;
							sender?: string;
							text?: string;
						};
						self.fireIncoming(
							body.chat_id ?? "default",
							body.sender ?? "ws-user",
							body.text ?? "",
						);
						return Response.json({ ok: true });
					})();
				}

				return new Response("not found", { status: 404 });
			},

			websocket: {
				open(ws: import("bun").ServerWebSocket<WsData>) {
					self.addClient(ws.data.chatId, ws);
				},

				message(
					ws: import("bun").ServerWebSocket<WsData>,
					raw: string | Buffer,
				) {
					const text = typeof raw === "string" ? raw : raw.toString("utf-8");
					let parsed: {
						type?: string;
						chat_id?: string;
						sender?: string;
						text?: string;
					};
					try {
						parsed = JSON.parse(text);
					} catch {
						return; // Ignore malformed frames.
					}
					if (parsed.type !== "send") return;

					// Re-key the connection if chat_id changed.
					const chatId = parsed.chat_id ?? "default";
					if (chatId !== ws.data.chatId) {
						self.removeClient(ws.data.chatId, ws);
						ws.data.chatId = chatId;
						self.addClient(chatId, ws);
					}

					self.fireIncoming(
						chatId,
						parsed.sender ?? "ws-user",
						parsed.text ?? "",
					);
				},

				close(ws: import("bun").ServerWebSocket<WsData>) {
					self.removeClient(ws.data.chatId, ws);
				},
			},
		};

		this.server = Bun.serve(serveOpts);
		const addr = this.config.unix
			? this.config.unix
			: `${this.server.hostname}:${this.server.port}`;
		log.info(`listening on ${addr}`);

		// Block until stop() is called.
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
		const set = this.clients.get(reply.chatId);
		if (!set || set.size === 0) return;
		const frame = JSON.stringify({
			type: "reply",
			chat_id: reply.chatId,
			text: reply.text,
			...(reply.meta ? { meta: reply.meta } : {}),
		});
		for (const ws of set) {
			ws.send(frame);
		}
	}

	startTyping(chatId: string): TypingHandle | null {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return null;
		const frame = JSON.stringify({ type: "typing", chat_id: chatId });
		const send = () => {
			const current = this.clients.get(chatId);
			if (!current) return;
			for (const ws of current) ws.send(frame);
		};
		send();
		const timer = setInterval(send, 4_000);
		return { stop: () => clearInterval(timer) };
	}

	async stop(): Promise<void> {
		for (const set of this.clients.values()) {
			for (const ws of set) {
				ws.close(1000, "shutdown");
			}
		}
		this.clients.clear();
		this.server?.stop();
		this.server = null;
	}

	// ── Internals ───────────────────────────────────────────────────

	private addClient(
		chatId: string,
		ws: import("bun").ServerWebSocket<WsData>,
	): void {
		let set = this.clients.get(chatId);
		if (!set) {
			set = new Set();
			this.clients.set(chatId, set);
		}
		set.add(ws);
	}

	private removeClient(
		chatId: string,
		ws: import("bun").ServerWebSocket<WsData>,
	): void {
		const set = this.clients.get(chatId);
		if (!set) return;
		set.delete(ws);
		if (set.size === 0) this.clients.delete(chatId);
	}

	private fireIncoming(chatId: string, sender: string, text: string): void {
		this.onMessage?.({
			channel: "ws",
			chatId,
			sender,
			text,
			timestamp: Date.now(),
			isMentioned: true,
			metadata: {},
			images: [],
		});
	}
}
