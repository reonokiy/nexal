/**
 * Svelte 5 reactive wrapper around the shared NexalChatClient.
 * The transport / protocol logic lives in `@nexal/chat-client` — this
 * file only translates the client's events into runes-backed state
 * (messages, status, typing) consumed by the UI.
 */
import { NexalChatClient, type Status, type CommandInfo } from "@nexal/chat-client";

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
	initialAuthToken = "",
) {
	const client = new NexalChatClient({
		url: initialUrl,
		chatId,
		sender,
		reconnectDelayMs: 0, // web stays manual; status drives the UI
		authToken: initialAuthToken,
	});

	let url = $state(initialUrl);
	let status = $state<Status>("idle");
	let typing = $state(false);
	const messages = $state<Message[]>([]);

	// command_result events whose name matches a positive count are
	// consumed by an awaiting caller (settings page, etc.) and should
	// not bubble up as a system note in the chat transcript.
	const mutedCounts = new Map<string, number>();

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
			case "auth_ok":
				pushSystem(`authenticated as ${ev.email ?? ev.userId}`);
				break;
			case "auth_error":
				pushSystem(`authentication failed: ${ev.error}`);
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
			case "command_result": {
				const count = mutedCounts.get(ev.name) ?? 0;
				if (count > 0) {
					mutedCounts.set(ev.name, count - 1);
					break;
				}
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

	/**
	 * Send a slash command and await the matching command_result.
	 * Resolves on the first command_result whose `name` matches.
	 * Bypasses the chat history entirely — does not push system notes.
	 */
	function runCommandAwait(
		name: string,
		args: string[] = [],
		timeoutMs = 5000,
	): Promise<{ text?: string; error?: string; data?: unknown }> {
		return new Promise((resolve, reject) => {
			mutedCounts.set(name, (mutedCounts.get(name) ?? 0) + 1);
			const off = client.on((ev) => {
				if (ev.type !== "command_result" || ev.name !== name) return;
				cleanup();
				resolve({ text: ev.text, error: ev.error, data: ev.data });
			});
			const timer = setTimeout(() => {
				cleanup();
				reject(new Error(`/${name} timed out`));
			}, timeoutMs);
			function cleanup() {
				off();
				clearTimeout(timer);
			}
			if (!client.sendCommand(name, args)) {
				const c = mutedCounts.get(name) ?? 0;
				if (c > 0) mutedCounts.set(name, c - 1);
				cleanup();
				reject(new Error("not connected"));
			}
		});
	}

	/** Fetch available commands from the server. */
	function listCommands(): Promise<CommandInfo[]> {
		return client.listCommands();
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
		get authToken() {
			return client.token ?? "";
		},
		set authToken(v: string) {
			client.token = v;
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
		runCommandAwait,
		listCommands,
	};
}

export type Chat = ReturnType<typeof createChat>;
