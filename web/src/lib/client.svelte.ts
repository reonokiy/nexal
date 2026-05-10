/**
 * Svelte 5 reactive wrapper around the shared NexalChatClient.
 * The transport / protocol logic lives in `@nexal/chat-client` — this
 * file only translates the client's events into runes-backed state
 * (messages, status, typing) consumed by the UI.
 */
import { NexalChatClient, type Status } from "@nexal/chat-client";

export type Role = "user" | "agent" | "system";

export interface Message {
	id: number;
	streamId?: string;
	role: Role;
	text: string;
	ts: number;
	streaming?: boolean;
}

let nextId = 1;

export function createChat(
	initialUrl: string,
	chatId = "default",
	sender = "web-user",
) {
	const client = new NexalChatClient({
		url: initialUrl,
		chatId,
		sender,
		reconnectDelayMs: 0, // web stays manual; status drives the UI
	});

	let url = $state(initialUrl);
	let status = $state<Status>("idle");
	let typing = $state(false);
	const messages = $state<Message[]>([]);

	let typingTimer: ReturnType<typeof setTimeout> | null = null;
	function setTyping(on: boolean) {
		typing = on;
		if (typingTimer) clearTimeout(typingTimer);
		if (on) typingTimer = setTimeout(() => (typing = false), 6000);
	}

	function pushSystem(text: string) {
		messages.push({ id: nextId++, role: "system", text, ts: Date.now() });
	}

	function findStreaming(streamId: string): Message | undefined {
		for (let i = messages.length - 1; i >= 0; i--) {
			if (messages[i]!.streamId === streamId) return messages[i];
		}
		return undefined;
	}

	client.on((ev) => {
		switch (ev.type) {
			case "open":
				status = client.status;
				pushSystem(`connected to ${url}`);
				break;
			case "close":
				status = client.status;
				pushSystem("disconnected");
				break;
			case "error":
				pushSystem("socket error");
				break;
			case "reply":
				setTyping(false);
				messages.push({
					id: nextId++,
					role: "agent",
					text: ev.text,
					ts: Date.now(),
				});
				break;
			case "reply_chunk": {
				setTyping(false);
				const existing = findStreaming(ev.messageId);
				if (existing) existing.text += ev.delta;
				else
					messages.push({
						id: nextId++,
						streamId: ev.messageId,
						role: "agent",
						text: ev.delta,
						ts: Date.now(),
						streaming: true,
					});
				break;
			}
			case "reply_end": {
				const existing = findStreaming(ev.messageId);
				if (existing) existing.streaming = false;
				break;
			}
			case "typing":
				setTyping(true);
				break;
			case "command_result":
				messages.push({
					id: nextId++,
					role: "system",
					text: ev.error
						? `/${ev.name} error: ${ev.error}`
						: `/${ev.name}: ${ev.text ?? ""}`,
					ts: Date.now(),
				});
				break;
		}
	});

	function connect(target?: string) {
		if (target) {
			url = target;
			client.url = target;
		}
		status = "connecting";
		client.connect();
	}

	function disconnect() {
		client.disconnect();
	}

	function sendText(text: string) {
		const trimmed = text.trim();
		if (!trimmed) return;
		const ok = trimmed.startsWith("/")
			? (() => {
					const [head, ...args] = trimmed.slice(1).split(/\s+/);
					return client.sendCommand(head!, args);
				})()
			: client.sendText(trimmed);
		if (!ok) {
			pushSystem("not connected");
			return;
		}
		messages.push({
			id: nextId++,
			role: "user",
			text: trimmed,
			ts: Date.now(),
		});
	}

	/** Send a slash command without echoing a "user" bubble. */
	function runCommand(name: string, args: string[] = []): boolean {
		const ok = client.sendCommand(name, args);
		if (!ok) pushSystem("not connected");
		return ok;
	}

	return {
		get url() {
			return url;
		},
		set url(v: string) {
			url = v;
			client.url = v;
		},
		get status() {
			return status;
		},
		get typing() {
			return typing;
		},
		get messages() {
			return messages;
		},
		connect,
		disconnect,
		sendText,
		runCommand,
	};
}

export type Chat = ReturnType<typeof createChat>;
