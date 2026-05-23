/**
 * NexalChatClient — environment-agnostic WebSocket client for the
 * nexal ws channel. Works in any runtime that exposes a global
 * `WebSocket` constructor (browsers, Bun, Node 22+).
 *
 * Shape:
 *   - Subscribe via `on(listener)`; receives a tagged-union event for
 *     every wire frame plus connection lifecycle (`open`/`close`/`error`).
 *   - `connect({ autoReconnect: true })` — auto-retries on close.
 *   - `sendText` / `sendCommand` — typed helpers; both no-op return
 *     `false` when the socket isn't open.
 */
import type {
	ClientFrame,
	ImageBlock,
	ReplyMetadata,
	ServerFrame,
	CommandInfo,
} from "./protocol";
import { ClientFrameType, ServerFrameType } from "./protocol";
import { encodeFrame, decodeFrame } from "./codec";

// ── Constants ──────────────────────────────────────────────────────

const DEFAULT_CHAT_ID = "default";
const DEFAULT_SENDER = "client";
const DEFAULT_RECONNECT_DELAY_MS = 2_000;

const WS_STATE_OPEN = 1;

// ── Types ──────────────────────────────────────────────────────────

export type Status = "idle" | "connecting" | "open" | "closed";

export interface NexalChatClientOptions {
	url: string;
	/** Default chat_id used by send helpers when one isn't passed. */
	chatId?: string;
	/** Default sender used by send helpers when one isn't passed. */
	sender?: string;
	/** Auto-reconnect delay in ms (only applied when `autoReconnect` is set). */
	reconnectDelayMs?: number;
	/** Supabase JWT access token sent in auth frame on connect. */
	authToken?: string;
}

export type ChatEvent =
	| { type: "open" }
	| { type: "close" }
	| { type: "error"; error: unknown }
	| { type: "auth_ok"; userId: string; email?: string }
	| { type: "auth_error"; error: string }
	| { type: "reply"; text: string; metadata?: ReplyMetadata }
	| { type: "reply_chunk"; messageId: string; delta: string }
	| { type: "reply_end"; messageId: string }
	| { type: "typing" }
	| {
			type: "command_result";
			name: string;
			text?: string;
			error?: string;
			data?: unknown;
	  };

export type ChatListener = (event: ChatEvent) => void;

// ── Handler map for server frames ──────────────────────────────────

type FrameHandler = (frame: ServerFrame) => void;

const NOOP_HANDLER: FrameHandler = () => {};

// ── Client ─────────────────────────────────────────────────────────

export class NexalChatClient {
	private ws: WebSocket | null = null;
	private statusValue: Status = "idle";
	private readonly listeners = new Set<ChatListener>();
	private urlValue: string;
	private readonly chatId: string;
	private readonly sender: string;
	private readonly reconnectDelayMs: number;
	private autoReconnect = false;
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private authToken: string | undefined;
	private authOk = false;
	private pendingListCommands: {
		resolve: (cmds: CommandInfo[]) => void;
		reject: (err: Error) => void;
		timer: ReturnType<typeof setTimeout>;
	} | null = null;

	constructor(opts: NexalChatClientOptions) {
		this.urlValue = opts.url;
		this.chatId = opts.chatId ?? DEFAULT_CHAT_ID;
		this.sender = opts.sender ?? DEFAULT_SENDER;
		this.reconnectDelayMs = opts.reconnectDelayMs ?? DEFAULT_RECONNECT_DELAY_MS;
		this.authToken = opts.authToken;
	}

	get url(): string {
		return this.urlValue;
	}

	set url(v: string) {
		this.urlValue = v;
	}

	get status(): Status {
		return this.statusValue;
	}

	get token(): string | undefined {
		return this.authToken;
	}

	set token(v: string | undefined) {
		this.authToken = v;
		if (v && this.ws?.readyState === WS_STATE_OPEN && !this.authOk) {
			this.sendAuth();
		}
	}

	on(listener: ChatListener): () => void {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	connect(opts: { autoReconnect?: boolean } = {}): void {
		this.autoReconnect = opts.autoReconnect ?? false;
		this.authOk = false;
		this.closeSocket();
		this.statusValue = "connecting";

		let socket: WebSocket;
		try {
			socket = new WebSocket(this.urlValue);
		} catch (err) {
			this.statusValue = "closed";
			this.emit({ type: "error", error: err });
			this.scheduleReconnect();
			return;
		}
		this.ws = socket;
		socket.binaryType = "arraybuffer";

		socket.onopen = () => {
			if (this.ws !== socket) return;
			this.statusValue = "open";
			this.emit({ type: "open" });
			this.sendAuth();
		};
		socket.onerror = (e) => this.emit({ type: "error", error: e });
		socket.onclose = () => {
			if (this.ws !== socket) return;
			this.statusValue = "closed";
			this.ws = null;
			this.emit({ type: "close" });
			this.scheduleReconnect();
		};
		socket.onmessage = (ev: MessageEvent) => {
			if (this.ws !== socket) return;
			this.dispatch(ev.data);
		};
	}

	disconnect(): void {
		this.autoReconnect = false;
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		this.closeSocket();
	}

	sendText(
		text: string,
		images?: ImageBlock[],
		opts?: { chatId?: string; sender?: string },
	): boolean {
		return this.sendFrame({
			type: ClientFrameType.Send,
			chat_id: opts?.chatId ?? this.chatId,
			sender: opts?.sender ?? this.sender,
			text,
			...(images && images.length ? { images } : {}),
		});
	}

	sendCommand(
		name: string,
		args: string[] = [],
		opts?: { chatId?: string; sender?: string },
	): boolean {
		return this.sendFrame({
			type: ClientFrameType.Command,
			chat_id: opts?.chatId ?? this.chatId,
			sender: opts?.sender ?? this.sender,
			name,
			args,
		});
	}

	/**
	 * Request the list of available commands from the server.
	 * Returns a promise that resolves with the command metadata.
	 */
	listCommands(timeoutMs = 5_000): Promise<CommandInfo[]> {
		return new Promise((resolve, reject) => {
			if (!this.ws || this.ws.readyState !== WS_STATE_OPEN) {
				reject(new Error("not connected"));
				return;
			}
			if (!this.authOk) {
				reject(new Error("not authenticated"));
				return;
			}
			// Reject any existing pending request before creating a new one.
			if (this.pendingListCommands) {
				clearTimeout(this.pendingListCommands.timer);
				this.pendingListCommands.reject(new Error("superseded by new listCommands call"));
				this.pendingListCommands = null;
			}
			const timer = setTimeout(() => {
				this.pendingListCommands = null;
				reject(new Error("list_commands timed out"));
			}, timeoutMs);
			this.pendingListCommands = { resolve, reject, timer };
			this.ws.send(encodeFrame({ type: ClientFrameType.ListCommands }));
		});
	}

	private sendFrame(frame: ClientFrame): boolean {
		if (!this.ws || this.ws.readyState !== WS_STATE_OPEN) return false;
		if (!this.authOk && frame.type !== ClientFrameType.Auth) {
			return false;
		}
		this.ws.send(encodeFrame(frame));
		return true;
	}

	private sendAuth(): void {
		if (!this.authToken || this.authOk) return;
		this.sendFrame({ type: ClientFrameType.Auth, token: this.authToken });
	}

	private dispatch(raw: unknown): void {
		let frame: ServerFrame;
		try {
			frame = decodeFrame(raw) as ServerFrame;
		} catch {
			return;
		}

		const handler = this.frameHandlers[frame.type] ?? NOOP_HANDLER;
		handler(frame);
	}

	private readonly frameHandlers: Record<ServerFrameType, FrameHandler> = {
		[ServerFrameType.AuthOk]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.AuthOk }>;
			this.authOk = true;
			this.emit({ type: "auth_ok", userId: f.user_id, email: f.email });
		},
		[ServerFrameType.AuthError]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.AuthError }>;
			this.emit({ type: "auth_error", error: f.error });
		},
		[ServerFrameType.Reply]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.Reply }>;
			this.emit({ type: "reply", text: f.text, metadata: f.metadata });
		},
		[ServerFrameType.ReplyChunk]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.ReplyChunk }>;
			this.emit({
				type: "reply_chunk",
				messageId: f.message_id,
				delta: f.delta,
			});
		},
		[ServerFrameType.ReplyEnd]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.ReplyEnd }>;
			this.emit({ type: "reply_end", messageId: f.message_id });
		},
		[ServerFrameType.Typing]: () => {
			this.emit({ type: "typing" });
		},
		[ServerFrameType.CommandResult]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.CommandResult }>;
			this.emit({
				type: "command_result",
				name: f.name,
				text: f.text,
				error: f.error,
				data: f.data,
			});
		},
		[ServerFrameType.ListCommandsResult]: (frame) => {
			const f = frame as Extract<ServerFrame, { type: typeof ServerFrameType.ListCommandsResult }>;
			if (this.pendingListCommands) {
				const { resolve, timer } = this.pendingListCommands;
				this.pendingListCommands = null;
				clearTimeout(timer);
				resolve(f.commands);
			}
		},
	};

	private emit(ev: ChatEvent): void {
		for (const l of this.listeners) {
			try {
				l(ev);
			} catch {
				/* listener errors are swallowed to keep the socket healthy */
			}
		}
	}

	private closeSocket(): void {
		this.authOk = false;
		if (this.pendingListCommands) {
			clearTimeout(this.pendingListCommands.timer);
			this.pendingListCommands.reject(new Error("disconnected"));
			this.pendingListCommands = null;
		}
		if (this.ws) {
			try {
				this.ws.close();
			} catch {
				/* ok */
			}
			this.ws = null;
		}
	}

	private scheduleReconnect(): void {
		if (!this.autoReconnect || this.reconnectDelayMs <= 0) return;
		if (this.reconnectTimer) return;
		this.reconnectTimer = setTimeout(() => {
			this.reconnectTimer = null;
			this.connect({ autoReconnect: true });
		}, this.reconnectDelayMs);
	}
}
