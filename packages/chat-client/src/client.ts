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
} from "./protocol";

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

	constructor(opts: NexalChatClientOptions) {
		this.urlValue = opts.url;
		this.chatId = opts.chatId ?? "default";
		this.sender = opts.sender ?? "client";
		this.reconnectDelayMs = opts.reconnectDelayMs ?? 2000;
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

	on(listener: ChatListener): () => void {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	connect(opts: { autoReconnect?: boolean } = {}): void {
		this.autoReconnect = opts.autoReconnect ?? false;
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

		socket.onopen = () => {
			this.statusValue = "open";
			this.emit({ type: "open" });
			this.sendAuth();
		};
		socket.onerror = (e) => this.emit({ type: "error", error: e });
		socket.onclose = () => {
			this.statusValue = "closed";
			this.ws = null;
			this.emit({ type: "close" });
			this.scheduleReconnect();
		};
		socket.onmessage = (ev: MessageEvent) => this.dispatch(ev.data);
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
			type: "send",
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
			type: "command",
			chat_id: opts?.chatId ?? this.chatId,
			sender: opts?.sender ?? this.sender,
			name,
			args,
		});
	}

	private sendFrame(frame: ClientFrame): boolean {
		if (!this.ws || this.ws.readyState !== 1 /* OPEN */) return false;
		if (!this.authOk && frame.type !== "auth") {
			// Queue instead of dropping? For simplicity, drop non-auth frames before auth.
			return false;
		}
		this.ws.send(JSON.stringify(frame));
		return true;
	}

	private sendAuth(): void {
		if (!this.authToken || this.authOk) return;
		this.sendFrame({ type: "auth", token: this.authToken });
	}

	private dispatch(raw: unknown): void {
		const text =
			typeof raw === "string"
				? raw
				: raw instanceof ArrayBuffer
				  ? new TextDecoder().decode(raw)
				  : (raw as { toString(): string }).toString();
		let frame: ServerFrame;
		try {
			frame = JSON.parse(text);
		} catch {
			return;
		}
		switch (frame.type) {
			case "auth_ok":
				this.authOk = true;
				this.emit({
					type: "auth_ok",
					userId: frame.user_id,
					email: frame.email,
				});
				return;
			case "auth_error":
				this.emit({ type: "auth_error", error: frame.error });
				return;
			case "reply":
				this.emit({
					type: "reply",
					text: frame.text,
					metadata: frame.metadata,
				});
				return;
			case "reply_chunk":
				this.emit({
					type: "reply_chunk",
					messageId: frame.message_id,
					delta: frame.delta,
				});
				return;
			case "reply_end":
				this.emit({ type: "reply_end", messageId: frame.message_id });
				return;
			case "typing":
				this.emit({ type: "typing" });
				return;
			case "command_result":
				this.emit({
					type: "command_result",
					name: frame.name,
					text: frame.text,
					error: frame.error,
					data: frame.data,
				});
				return;
		}
	}

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
