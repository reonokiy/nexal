/**
 * Channel abstraction — ports the Rust `Channel` trait from
 * `crates/channel-core/src/lib.rs` to TypeScript.
 *
 * A Channel is a one-way input source: it pulls messages from some
 * external surface (Telegram, Discord, HTTP, cron, heartbeat) and
 * hands them to the AgentPool as `IncomingMessage`s. Replies go back
 * through `Channel.send`.
 *
 * Field names mirror Rust `nexal_channel_core::IncomingMessage` (see
 * `crates/channel-core/src/message.rs`):
 *   - `sender`     (Rust) ↔ `sender`     (TS)
 *   - `isMentioned` (Rust `is_mentioned`) ↔ `isMentioned` (TS)
 *   - `chatId`     (Rust `chat_id`)     ↔ `chatId`     (TS)
 */

export interface ImageAttachment {
	/** Raw image bytes (Uint8Array) or base64 data URL. */
	data: Uint8Array | string;
	/** MIME type, e.g. "image/jpeg". */
	mimeType: string;
	/** Original filename (best-effort). */
	filename: string;
}

export interface IncomingMessage {
	/** Channel identifier, e.g. "telegram", "http", "cron". */
	channel: string;
	/** Stable conversation key on this channel (Telegram chat_id, etc.). */
	chatId: string;
	/** Human-readable sender name (username or display name). */
	sender: string;
	/** Text content (or synthesized text for media-only messages). */
	text: string;
	/** Unix milliseconds timestamp of the upstream event. */
	timestamp: number;
	/**
	 * True if the bot was explicitly addressed (DM, @-mention, reply to bot).
	 * Channels that can't tell should default to `true` (treat as addressed).
	 */
	isMentioned: boolean;
	/** Channel-specific metadata (message_id, is_admin, album id, …). */
	metadata: Record<string, unknown>;
	/** Attached images (downloaded bytes). */
	images: ImageAttachment[];
}

export function sessionKey(msg: IncomingMessage): string {
	return `${msg.channel}:${msg.chatId}`;
}

export interface OutgoingReply {
	chatId: string;
	text: string;
	images?: ImageAttachment[];
	/** If set, quote-reply to this upstream message id (channel-specific). */
	replyTo?: string;
	meta?: Record<string, unknown>;
}

/**
 * Opaque handle for a "typing" indicator. `stop()` cancels it.
 * Channels that don't support typing indicators return `null` from
 * `startTyping` and we simply don't animate.
 */
export interface TypingHandle {
	stop(): void;
}

export interface Channel {
	/** Display name, e.g. "telegram". */
	readonly name: string;

	/**
	 * Start receiving. `onMessage` must be safe to call from any task
	 * and non-blocking (it hands off to the AgentPool, which queues).
	 * The returned promise resolves when the channel voluntarily exits
	 * (or rejects on fatal error). Most channels run forever until
	 * `stop()` is called.
	 */
	start(onMessage: (msg: IncomingMessage) => void): Promise<void>;

	/** Deliver a reply. Safe to call concurrently. */
	send(reply: OutgoingReply): Promise<void>;

	/** Optional typing-indicator support. */
	startTyping?(chatId: string): TypingHandle | null;

	/** Graceful shutdown. */
	stop(): Promise<void>;
}
