/**
 * Telegram channel — TS port of `crates/channel-telegram/src/lib.rs`.
 *
 * Uses Telegram Bot API `getUpdates` long-polling directly over
 * `fetch` (no SDK). Supports:
 *   - text / photo (downloaded) / document (logged) / sticker (thumb) /
 *     video / voice / animation / audio
 *   - media-group (album) buffering with a 1500ms timer
 *   - allow_chats + allow_from ACL (either hit allows)
 *   - detect_mention: DM, reply-to-bot, or "@<bot>" in text
 *   - sender extraction for Channel_Bot / GroupAnonymousBot forwards
 *   - sendMessage + sendChatAction (typing) for outgoing
 *
 * Deviations from the Rust impl:
 *   - teloxide's Dispatcher is replaced by a hand-rolled long-poll
 *     loop. `getUpdates` timeout is 30s; we keep offset across calls.
 *   - Admin detection (the Rust code tags the message with is_admin
 *     from NexalConfig.is_admin) is skipped for now — goes onto
 *     config-loader task.
 */

import type {
	Channel,
	ImageAttachment,
	IncomingMessage,
	OutgoingReply,
	TypingHandle,
} from "./types.ts";
import type { CommandRegistry } from "../commands/registry.ts";
import { createLog } from "../log.ts";

const log = createLog("telegram");

const TG = "https://api.telegram.org";

export interface TelegramChannelConfig {
	botToken: string;
	/** Allowed usernames (e.g. "alice"). Empty = no filter. */
	allowFrom?: string[];
	/** Allowed chat ids (string form). Empty = no filter. */
	allowChats?: string[];
	/** Long-poll timeout in seconds (passed to getUpdates). */
	longPollTimeoutSec?: number;
	/** Shared command registry for slash commands. */
	commands?: CommandRegistry;
}

interface TelegramChat {
	id: number;
	type: string;
	username?: string;
	title?: string;
}

interface TelegramUser {
	id: number;
	is_bot?: boolean;
	username?: string;
	first_name?: string;
	last_name?: string;
}

interface TelegramPhotoSize {
	file_id: string;
	file_unique_id: string;
	width: number;
	height: number;
	file_size?: number;
}

interface TelegramSticker {
	file_id: string;
	file_unique_id: string;
	is_animated: boolean;
	is_video: boolean;
	emoji?: string;
	set_name?: string;
	thumbnail?: TelegramPhotoSize;
}

interface TelegramMessage {
	message_id: number;
	from?: TelegramUser;
	sender_chat?: TelegramChat;
	chat: TelegramChat;
	date: number;
	media_group_id?: string;
	text?: string;
	caption?: string;
	author_signature?: string;
	reply_to_message?: TelegramMessage;
	forward_from?: TelegramUser;
	photo?: TelegramPhotoSize[];
	document?: { file_id: string; file_name?: string; mime_type?: string };
	sticker?: TelegramSticker;
	video?: { file_id: string; file_name?: string };
	voice?: { file_id: string };
	animation?: { file_id: string };
	audio?: { file_id: string; title?: string };
}

interface TelegramUpdate {
	update_id: number;
	message?: TelegramMessage;
	edited_message?: TelegramMessage;
	channel_post?: TelegramMessage;
}

interface PendingGroup {
	items: Array<{ text: string; images: ImageAttachment[]; msg: TelegramMessage }>;
	timer: ReturnType<typeof setTimeout>;
}

export class TelegramChannel implements Channel {
	readonly name = "telegram";
	private stopping = false;
	private botUsername = "";
	private offset = 0;
	private readonly mediaGroups = new Map<string, PendingGroup>();
	private loopTask: Promise<void> | null = null;

	constructor(private readonly config: TelegramChannelConfig) {}

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		if (!this.config.botToken) throw new Error("telegram: botToken is required");

		this.botUsername = (await this.getMe()).username ?? "";
		log.info(`logged in as @${this.botUsername || "<unknown>"}, polling for updates`);

		// Register slash commands with Telegram so they appear in the
		// bot command menu.
		if (this.config.commands) {
			const cmds = this.config.commands.list().map((c) => ({
				command: c.name,
				description: c.description.slice(0, 256),
			}));
			await this.apiCall("setMyCommands", { commands: cmds }).catch((err) =>
				log.error("failed to register bot commands with Telegram", err),
			);
		}

		this.loopTask = this.longPollLoop(onMessage);
		await this.loopTask;
	}

	async send(reply: OutgoingReply): Promise<void> {
		await this.apiCall("sendMessage", {
			chat_id: reply.chatId,
			text: reply.text,
			...(reply.replyTo ? { reply_parameters: { message_id: Number(reply.replyTo) } } : {}),
		});
	}

	startTyping(chatId: string): TypingHandle | null {
		let stopped = false;
		const tick = async () => {
			while (!stopped) {
				try {
					await this.apiCall("sendChatAction", { chat_id: chatId, action: "typing" });
				} catch {
					// swallow — the loop will retry
				}
				await new Promise<void>((r) => setTimeout(r, 4000));
			}
		};
		void tick();
		return {
			stop() {
				stopped = true;
			},
		};
	}

	async stop(): Promise<void> {
		this.stopping = true;
		for (const g of this.mediaGroups.values()) clearTimeout(g.timer);
		this.mediaGroups.clear();
		await this.loopTask?.catch(() => undefined);
	}

	// ---------------------------------------------------------------------
	// Internals
	// ---------------------------------------------------------------------

	private async longPollLoop(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		const timeout = this.config.longPollTimeoutSec ?? 30;
		while (!this.stopping) {
			let updates: TelegramUpdate[];
			try {
				updates = await this.apiCall<TelegramUpdate[]>("getUpdates", {
					offset: this.offset,
					timeout,
					allowed_updates: ["message", "channel_post", "edited_message"],
				});
			} catch (err) {
				log.error("getUpdates failed, retrying in 2s", err);
				await new Promise((r) => setTimeout(r, 2_000));
				continue;
			}
			for (const up of updates) {
				this.offset = up.update_id + 1;
				const msg = up.message ?? up.channel_post ?? up.edited_message;
				if (!msg) continue;
				await this.handleMessage(msg, onMessage);
			}
		}
	}

	private async handleMessage(
		msg: TelegramMessage,
		onMessage: (m: IncomingMessage) => void,
	): Promise<void> {
		const chatId = String(msg.chat.id);
		const { username, userId } = extractSender(msg);

		const allowChat = !this.config.allowChats?.length || this.config.allowChats.includes(chatId);
		const isChannelBotForward = msg.from?.username === "Channel_Bot" || msg.from?.username === "GroupAnonymousBot";
		const allowUser =
			isChannelBotForward ||
			!this.config.allowFrom?.length ||
			this.config.allowFrom.includes(username);

		if (!allowChat && !allowUser) {
			await this.apiCall("sendMessage", {
				chat_id: chatId,
				text: `⚠️ Not authorized.\nchat_id: ${chatId}\nuser: @${username} (id: ${userId})`,
			}).catch(() => undefined);
			return;
		}

		const { text, images } = await this.extractContent(msg);
		if (!text && images.length === 0) return;

		// Intercept slash commands — Telegram sends "/cmd" or "/cmd@botname".
		const cmds = this.config.commands;
		if (cmds && text.startsWith("/")) {
			const cmdMatch = text.match(/^\/(\w+)(?:@\S+)?\s*(.*)/s);
			if (cmdMatch) {
				const name = cmdMatch[1]!;
				const argStr = cmdMatch[2]?.trim() ?? "";
				const args = argStr ? argStr.split(/\s+/) : [];
				if (cmds.has(name)) {
					const result = await cmds
						.execute(name, { channel: "telegram", chatId, sender: username }, args)
						.catch((err) => {
							log.error(`command /${name} failed`, err);
							return { text: `Command failed: ${err instanceof Error ? err.message : err}` };
						});
					if (result) {
						await this.apiCall("sendMessage", {
							chat_id: chatId,
							text: result.text,
							...(msg.message_id ? { reply_parameters: { message_id: msg.message_id } } : {}),
						}).catch((err) => log.error(`failed to reply to /${name}`, err));
					}
					return;
				}
			}
		}

		// Media group (album) buffering.
		if (msg.media_group_id) {
			this.bufferAlbum(msg.media_group_id, { text, images, msg }, onMessage);
			return;
		}

		const isMentioned = this.detectMention(msg, text);
		const incoming: IncomingMessage = {
			channel: "telegram",
			chatId,
			sender: username,
			text,
			timestamp: msg.date * 1000,
			isMentioned,
			metadata: { message_id: msg.message_id, user_id: userId },
			images,
		};
		onMessage(incoming);
	}

	private bufferAlbum(
		groupId: string,
		item: { text: string; images: ImageAttachment[]; msg: TelegramMessage },
		onMessage: (m: IncomingMessage) => void,
	): void {
		const existing = this.mediaGroups.get(groupId);
		if (existing) {
			existing.items.push(item);
			return;
		}
		const timer = setTimeout(() => {
			const pending = this.mediaGroups.get(groupId);
			this.mediaGroups.delete(groupId);
			if (pending) this.dispatchAlbum(pending.items, onMessage);
		}, 1500);
		this.mediaGroups.set(groupId, { items: [item], timer });
	}

	private dispatchAlbum(
		items: Array<{ text: string; images: ImageAttachment[]; msg: TelegramMessage }>,
		onMessage: (m: IncomingMessage) => void,
	): void {
		if (items.length === 0) return;
		const first = items[0]!.msg;
		const chatId = String(first.chat.id);
		const { username, userId } = extractSender(first);
		const combined = items
			.map((i) => i.text)
			.filter((t) => t.length > 0)
			.join("\n");
		const allImages = items.flatMap((i) => i.images);
		const text = combined || `[received album with ${allImages.length} image(s)]`;
		const isMentioned = this.detectMention(first, text);
		onMessage({
			channel: "telegram",
			chatId,
			sender: username,
			text,
			timestamp: first.date * 1000,
			isMentioned,
			metadata: { message_id: first.message_id, user_id: userId, album: true },
			images: allImages,
		});
	}

	private detectMention(msg: TelegramMessage, text: string): boolean {
		if (msg.chat.type === "private") return true;
		if (msg.reply_to_message?.from?.is_bot) return true;
		if (this.botUsername && text.includes(`@${this.botUsername}`)) return true;
		return false;
	}

	private async extractContent(
		msg: TelegramMessage,
	): Promise<{ text: string; images: ImageAttachment[] }> {
		if (msg.text) return { text: msg.text, images: [] };

		if (msg.photo && msg.photo.length > 0) {
			const largest = msg.photo[msg.photo.length - 1]!;
			const data = await this.downloadFile(largest.file_id).catch(() => null);
			const images: ImageAttachment[] =
				data === null
					? []
					: [
							{
								data: new Uint8Array(data),
								mimeType: "image/jpeg",
								filename: `${largest.file_unique_id}.jpg`,
							},
						];
			return { text: msg.caption ?? "", images };
		}

		if (msg.document) {
			const caption = msg.caption ?? "";
			const name = msg.document.file_name ?? "file";
			const mime = msg.document.mime_type ?? "";
			return {
				text: `${caption}\n[received document: ${name} (${mime}), file_id: ${msg.document.file_id}]`.trim(),
				images: [],
			};
		}

		if (msg.sticker) {
			const s = msg.sticker;
			const emoji = s.emoji ?? "";
			const set = s.set_name ? `, set: ${s.set_name}` : "";
			let images: ImageAttachment[] = [];
			// Prefer thumbnail for token efficiency.
			const target = s.thumbnail
				? { id: s.thumbnail.file_id, mime: "image/jpeg", ext: "jpg", uid: s.thumbnail.file_unique_id }
				: !s.is_animated && !s.is_video
					? { id: s.file_id, mime: "image/webp", ext: "webp", uid: s.file_unique_id }
					: null;
			if (target) {
				const data = await this.downloadFile(target.id).catch(() => null);
				if (data)
					images = [
						{
							data: new Uint8Array(data),
							mimeType: target.mime,
							filename: `${target.uid}.${target.ext}`,
						},
					];
			}
			return { text: `[sticker ${emoji}, file_id: ${s.file_id}${set}]`, images };
		}

		if (msg.voice) {
			return {
				text: `${msg.caption ?? ""}\n[received voice message, file_id: ${msg.voice.file_id}]`.trim(),
				images: [],
			};
		}
		if (msg.video) {
			return {
				text: `${msg.caption ?? ""}\n[received video: ${msg.video.file_name ?? "video"}, file_id: ${msg.video.file_id}]`.trim(),
				images: [],
			};
		}
		if (msg.animation) {
			return {
				text: `${msg.caption ?? ""}\n[received animation/gif, file_id: ${msg.animation.file_id}]`.trim(),
				images: [],
			};
		}
		if (msg.audio) {
			return {
				text: `${msg.caption ?? ""}\n[received audio: ${msg.audio.title ?? "audio"}, file_id: ${msg.audio.file_id}]`.trim(),
				images: [],
			};
		}
		return { text: "", images: [] };
	}

	private async getMe(): Promise<TelegramUser> {
		return this.apiCall<TelegramUser>("getMe", {});
	}

	private async downloadFile(fileId: string): Promise<Buffer> {
		const file = await this.apiCall<{ file_path: string }>("getFile", { file_id: fileId });
		const url = `${TG}/file/bot${this.config.botToken}/${file.file_path}`;
		const resp = await fetch(url);
		if (!resp.ok) throw new Error(`getFile download failed: ${resp.status}`);
		return Buffer.from(await resp.arrayBuffer());
	}

	private async apiCall<T>(method: string, params: Record<string, unknown>): Promise<T> {
		const url = `${TG}/bot${this.config.botToken}/${method}`;
		const resp = await fetch(url, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify(params),
		});
		const body = (await resp.json()) as { ok: boolean; result?: T; description?: string };
		if (!body.ok) throw new Error(`telegram ${method}: ${body.description ?? resp.status}`);
		return body.result as T;
	}
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

/**
 * Extract the real sender. For channel-forwarded messages, `msg.from`
 * is the "Channel_Bot" / "GroupAnonymousBot" placeholder; fall back to
 * author_signature, sender_chat, or forward_from for the real identity.
 * Ports `extract_sender` from channel-telegram/src/lib.rs.
 */
function extractSender(msg: TelegramMessage): { username: string; userId: string } {
	const fromUsername = msg.from?.username ?? "unknown";
	const fromId = msg.from?.id !== undefined ? String(msg.from.id) : "";

	if (fromUsername !== "Channel_Bot" && fromUsername !== "GroupAnonymousBot") {
		return { username: fromUsername, userId: fromId };
	}
	if (msg.author_signature) {
		return { username: msg.author_signature, userId: fromId };
	}
	if (msg.sender_chat) {
		const u = msg.sender_chat.username ?? msg.sender_chat.title ?? "";
		if (u) return { username: u, userId: String(msg.sender_chat.id) };
	}
	if (msg.forward_from) {
		const u = msg.forward_from.username ?? msg.forward_from.first_name ?? "unknown";
		return { username: u, userId: String(msg.forward_from.id) };
	}
	return { username: fromUsername, userId: fromId };
}
