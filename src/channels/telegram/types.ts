import type { ImageAttachment } from "../types.ts";
import type { CommandRegistry } from "../../commands/registry.ts";

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

export interface TelegramChat {
	id: number;
	type: string;
	username?: string;
	title?: string;
}

export interface TelegramUser {
	id: number;
	is_bot?: boolean;
	username?: string;
	first_name?: string;
	last_name?: string;
}

export interface TelegramPhotoSize {
	file_id: string;
	file_unique_id: string;
	width: number;
	height: number;
	file_size?: number;
}

export interface TelegramSticker {
	file_id: string;
	file_unique_id: string;
	is_animated: boolean;
	is_video: boolean;
	emoji?: string;
	set_name?: string;
	thumbnail?: TelegramPhotoSize;
}

export interface TelegramMessage {
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

export interface TelegramUpdate {
	update_id: number;
	message?: TelegramMessage;
	edited_message?: TelegramMessage;
	channel_post?: TelegramMessage;
}

export interface PendingGroup {
	items: Array<{ text: string; images: ImageAttachment[]; msg: TelegramMessage }>;
	timer: ReturnType<typeof setTimeout>;
}
