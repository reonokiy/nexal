/**
 * WebSocket channel — push-based replacement for the polling HTTP channel.
 *
 * Transport:
 *   - Default: Unix domain socket at `~/.nexal/nexal.sock` (local dev)
 *   - Fallback: TCP on configurable host:port
 *
 * Wire protocol: see `ws-protocol.ts` for typed frame definitions.
 *
 * The `fetch` handler also accepts `POST /send` for curl debugging,
 * same schema as the WsSendFrame.
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
import type {
	WsClientFrame,
	WsSendFrame,
	WsCommandFrame,
	WsReplyFrame,
	WsTypingFrame,
	WsCommandResultFrame,
	WsReplyChunkFrame,
	WsReplyEndFrame,
} from "./ws-protocol.ts";

const log = createLog("ws");

type BunServer = ReturnType<typeof Bun.serve>;

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

interface WsData {
	chatId: string;
	authed: boolean;
	userId?: string;
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

				// Health check for Fly.io probes.
				if (req.method === "GET" && url.pathname === "/health") {
					return new Response("ok", { status: 200 });
				}

				// WebSocket upgrade — any GET request.
				if (req.method === "GET" && req.headers.get("upgrade") === "websocket") {
					if (server.upgrade(req, { data: { chatId: "default", authed: false } as WsData })) {
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
					let frame: WsClientFrame & { type: string; token?: string };
					try {
						frame = JSON.parse(text);
					} catch {
						return;
					}

					// Auth enforcement: first frame must be "auth" if auth is enabled.
					if (isAuthEnabled() && !ws.data.authed) {
						if (frame.type !== "auth" || !frame.token) {
							ws.send(
								JSON.stringify({
									type: "auth_error",
									error: "authentication required — send auth frame first",
								}),
							);
							ws.close(4001, "auth required");
							return;
						}
						void verifySupabaseJwt(frame.token).then((user) => {
							if (user) {
								ws.data.authed = true;
								ws.data.userId = user.sub;
								ws.send(
									JSON.stringify({
										type: "auth_ok",
										user_id: user.sub,
										email: user.email,
									}),
								);
							} else {
								ws.send(
									JSON.stringify({
										type: "auth_error",
										error: "invalid or expired token",
									}),
								);
								ws.close(4001, "bad token");
							}
						});
						return;
					}

					// At this point frame is SendFrame | CommandFrame (not auth).
					const f = frame as WsSendFrame | WsCommandFrame;
					const chatId = f.chat_id ?? "default";
					if (chatId !== ws.data.chatId) {
						self.removeClient(ws.data.chatId, ws);
						ws.data.chatId = chatId;
						self.addClient(chatId, ws);
					}

					if (f.type === "command") {
						self.handleCommand(
							ws,
							chatId,
							f.sender ?? "ws-user",
							f.name!,
							f.args ?? [],
						);
						return;
					}

					if (f.type === "send") {
						self.fireIncoming(
							chatId,
							f.sender ?? "ws-user",
							f.text ?? "",
							f.images,
						);
					}
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
		return waitUntilStopped(() => !this.server);
	}

	async send(reply: OutgoingReply): Promise<void> {
		const set = this.clients.get(reply.chatId);
		if (!set || set.size === 0) return;
		const frame: WsReplyFrame = {
			type: "reply",
			chat_id: reply.chatId,
			text: reply.text,
			...(reply.metadata ? { metadata: reply.metadata as WsReplyFrame["metadata"] } : {}),
		};
		const json = JSON.stringify(frame);
		for (const ws of set) {
			ws.send(json);
		}
	}

	sendChunk(chatId: string, messageId: string, delta: string): void {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return;
		const frame: WsReplyChunkFrame = {
			type: "reply_chunk",
			chat_id: chatId,
			message_id: messageId,
			delta,
		};
		const json = JSON.stringify(frame);
		for (const ws of set) ws.send(json);
	}

	sendEnd(chatId: string, messageId: string): void {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return;
		const frame: WsReplyEndFrame = {
			type: "reply_end",
			chat_id: chatId,
			message_id: messageId,
		};
		const json = JSON.stringify(frame);
		for (const ws of set) ws.send(json);
	}

	startTyping(chatId: string): TypingHandle | null {
		const set = this.clients.get(chatId);
		if (!set || set.size === 0) return null;
		const frame: WsTypingFrame = { type: "typing", chat_id: chatId };
		const json = JSON.stringify(frame);
		const send = () => {
			const current = this.clients.get(chatId);
			if (!current) return;
			for (const ws of current) ws.send(json);
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

	private sendCommandResult(
		ws: import("bun").ServerWebSocket<WsData>,
		frame: WsCommandResultFrame,
	): void {
		ws.send(JSON.stringify(frame));
	}

	private handleCommand(
		ws: import("bun").ServerWebSocket<WsData>,
		chatId: string,
		sender: string,
		name: string,
		args: string[],
	): void {
		const cmds = this.config.commands;
		if (!cmds || !cmds.has(name)) {
			this.sendCommandResult(ws, {
				type: "command_result",
				chat_id: chatId,
				name,
				error: `unknown command: /${name}`,
			});
			return;
		}
		void cmds
			.execute(name, { channel: "ws", chatId, sender }, args)
			.then((result) => {
				this.sendCommandResult(ws, {
					type: "command_result",
					chat_id: chatId,
					name,
					text: result?.text ?? "",
					...(result && "data" in result ? { data: result.data } : {}),
				});
			})
			.catch((err) => {
				log.error(`command /${name} failed`, err);
				this.sendCommandResult(ws, {
					type: "command_result",
					chat_id: chatId,
					name,
					error: err instanceof Error ? err.message : String(err),
				});
			});
	}

	private fireIncoming(
		chatId: string,
		sender: string,
		text: string,
		images?: { data: string; mimeType: string }[],
	): void {
		this.onMessage?.({
			channel: "ws",
			chatId,
			sender,
			text,
			timestamp: Date.now(),
			isMentioned: true,
			metadata: {},
			images: images?.map((img) => ({
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
