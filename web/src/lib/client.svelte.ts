/**
 * Svelte 5 reactive wrapper around the @nexal/transport chat client.
 *
 * Connection lifecycle (open / auth / reconnect) lives here; the
 * transport package only provides the typed `createChatClient(peer)`
 * factory + `createWebSocketConnection` for plumbing. Events are
 * translated into runes-backed state (`messages`, `status`, `typing`)
 * consumed by the UI.
 */
import {
	createChatClient,
	createWebSocketConnection,
	type ChatClient,
	type CommandInfo,
	type ReplyMetadata,
	type WebSocketConnection,
} from "@nexal/transport";

export type Role = "user" | "agent" | "system";
export type Status = "idle" | "connecting" | "open" | "closed";

export interface Message {
	id: number;
	streamId?: string;
	role: Role;
	text: string;
	ts: number;
	streaming?: boolean;
	metadata?: ReplyMetadata;
}

let nextId = 1;
const HISTORY_PREFIX = "nexal.chat.history.";
const MAX_PERSISTED_MESSAGES = 1000;

export function createChat(
	initialUrl: string,
	chatIdValue = "default",
	senderValue = "web-user",
	initialAuthToken = "",
) {
	let url = $state(initialUrl);
	let status = $state<Status>("idle");
	let typing = $state(false);
	const historyKey = `${HISTORY_PREFIX}${chatIdValue}`;
	const restoredMessages = loadCachedMessages(historyKey);
	const messages = $state<Message[]>(restoredMessages);
	let authToken = initialAuthToken;
	let authOk = false;

	if (restoredMessages.length > 0) {
		nextId = Math.max(nextId, Math.max(...restoredMessages.map((m) => m.id)) + 1);
	}

	let ws: WebSocketConnection | null = null;
	let chat: ChatClient | null = null;
	let connectGeneration = 0;

	// command_result events whose name matches a positive count are
	// consumed by an awaiting caller (settings page, etc.) and should
	// not bubble up as a system note in the chat transcript.
	const mutedCounts = new Map<string, number>();
	const awaiters = new Set<(p: {
		name: string;
		text?: string;
		error?: string;
		data?: unknown;
	}) => void>();

	let typingTimer: ReturnType<typeof setTimeout> | null = null;
	let saveTimer: ReturnType<typeof setTimeout> | null = null;

	function scheduleSave() {
		if (saveTimer) clearTimeout(saveTimer);
		saveTimer = setTimeout(() => {
			saveTimer = null;
			saveCachedMessages(historyKey, messages);
		}, 120);
	}

	function setTyping(on: boolean) {
		typing = on;
		if (typingTimer) clearTimeout(typingTimer);
		if (on) typingTimer = setTimeout(() => (typing = false), 6000);
	}

	function pushSystem(text: string) {
		messages.push({ id: nextId++, role: "system", text, ts: Date.now() });
		scheduleSave();
	}

	function findStreaming(streamId: string): Message | undefined {
		for (let i = messages.length - 1; i >= 0; i--) {
			if (messages[i]!.streamId === streamId) return messages[i];
		}
		return undefined;
	}

	function wire(client: ChatClient): void {
		client.onReply(({ text, metadata }) => {
			setTyping(false);
			messages.push({
				id: nextId++,
				role: "agent",
				text,
				ts: Date.now(),
				metadata,
			});
			scheduleSave();
		});
		client.onTyping(() => setTyping(true));
		client.onReplyChunk(({ messageId, delta }) => {
			setTyping(false);
			const existing = findStreaming(messageId);
			if (existing) existing.text += delta;
			else
				messages.push({
					id: nextId++,
					streamId: messageId,
					role: "agent",
					text: delta,
					ts: Date.now(),
					streaming: true,
				});
			scheduleSave();
		});
		client.onReplyEnd(({ messageId }) => {
			const existing = findStreaming(messageId);
			if (existing) {
				existing.streaming = false;
				scheduleSave();
			}
		});
		client.onCommandResult(({ name, text, error, data }) => {
			for (const cb of awaiters) cb({ name, text, error, data });
			const count = mutedCounts.get(name) ?? 0;
			if (count > 0) {
				mutedCounts.set(name, count - 1);
				return;
			}
			messages.push({
				id: nextId++,
				role: "system",
				text: error ? `/${name} error: ${error}` : `/${name}: ${text ?? ""}`,
				ts: Date.now(),
			});
			scheduleSave();
		});
	}

	async function connect(target?: string) {
		if (target) url = target;
		const generation = ++connectGeneration;
		closeSocket();
		status = "connecting";

		let next: WebSocketConnection;
		try {
			next = await createWebSocketConnection(url, {
				onDisconnect: () => {
					if (generation !== connectGeneration) return;
					status = "closed";
					ws = null;
					chat = null;
					authOk = false;
					pushSystem("disconnected");
				},
			});
		} catch {
			if (generation !== connectGeneration) return;
			status = "closed";
			pushSystem("socket error");
			return;
		}
		if (generation !== connectGeneration) {
			next.connection.close();
			return;
		}

		ws = next;
		chat = createChatClient(next.connection);
		status = "open";
		wire(chat);
		pushSystem(`connected to ${url}`);

		if (authToken) await runAuth(chat);
	}

	async function runAuth(client: ChatClient): Promise<void> {
		if (!authToken || authOk) return;
		try {
			const { userId, email } = await client.authenticate({ token: authToken });
			authOk = true;
			pushSystem(`authenticated as ${email ?? userId}`);
		} catch (err) {
			authOk = false;
			pushSystem(
				`authentication failed: ${err instanceof Error ? err.message : String(err)}`,
			);
		}
	}

	function disconnect() {
		connectGeneration++;
		closeSocket();
	}

	function closeSocket() {
		authOk = false;
		if (ws) {
			try {
				ws.connection.close();
			} catch {
				/* ok */
			}
			ws = null;
			chat = null;
		}
	}

	function sendText(text: string) {
		const trimmed = text.trim();
		if (!trimmed) return;
		if (!chat || !authOk) {
			pushSystem("not connected");
			return;
		}
		if (trimmed.startsWith("/")) {
			const [head, ...args] = trimmed.slice(1).split(/\s+/);
			chat.command({
				chatId: chatIdValue,
				sender: senderValue,
				name: head!,
				args,
			});
		} else {
			chat.send({ chatId: chatIdValue, sender: senderValue, text: trimmed });
		}
		messages.push({ id: nextId++, role: "user", text: trimmed, ts: Date.now() });
		scheduleSave();
	}

	function clearMessages() {
		messages.length = 0;
		saveCachedMessages(historyKey, messages);
	}

	/** Send a slash command without echoing a "user" bubble. */
	function runCommand(name: string, args: string[] = []): boolean {
		if (!chat || !authOk) {
			pushSystem("not connected");
			return false;
		}
		chat.command({ chatId: chatIdValue, sender: senderValue, name, args });
		return true;
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
			if (!chat || !authOk) {
				reject(new Error("not connected"));
				return;
			}
			mutedCounts.set(name, (mutedCounts.get(name) ?? 0) + 1);
			const callback = (p: {
				name: string;
				text?: string;
				error?: string;
				data?: unknown;
			}) => {
				if (p.name !== name) return;
				cleanup();
				resolve({ text: p.text, error: p.error, data: p.data });
			};
			awaiters.add(callback);
			const timer = setTimeout(() => {
				cleanup();
				reject(new Error(`/${name} timed out`));
			}, timeoutMs);
			function cleanup() {
				awaiters.delete(callback);
				clearTimeout(timer);
			}
			chat.command({ chatId: chatIdValue, sender: senderValue, name, args });
		});
	}

	/** Fetch available commands from the server. */
	async function listCommands(): Promise<CommandInfo[]> {
		if (!chat || !authOk) throw new Error("not connected");
		const { commands } = await chat.listCommands();
		return commands;
	}

	return {
		get url() {
			return url;
		},
		set url(v: string) {
			url = v;
		},
		get status() {
			return status;
		},
		get authToken() {
			return authToken;
		},
		set authToken(v: string) {
			authToken = v;
			if (v && chat && !authOk) void runAuth(chat);
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
		clearMessages,
		runCommand,
		runCommandAwait,
		listCommands,
	};
}

export type Chat = ReturnType<typeof createChat>;

function loadCachedMessages(key: string): Message[] {
	try {
		if (typeof localStorage === "undefined") return [];
		const raw = localStorage.getItem(key);
		if (!raw) return [];
		const parsed = JSON.parse(raw) as unknown;
		if (!Array.isArray(parsed)) return [];
		return parsed.filter(isPersistableMessage).map((message) => ({
			...message,
			streaming: false,
			streamId: undefined,
		}));
	} catch {
		return [];
	}
}

function saveCachedMessages(key: string, messages: readonly Message[]) {
	try {
		if (typeof localStorage === "undefined") return;
		const persisted = messages
			.filter((message) => message.role !== "system")
			.slice(-MAX_PERSISTED_MESSAGES)
			.map((message) => ({
				...message,
				streaming: false,
				streamId: undefined,
			}));
		localStorage.setItem(key, JSON.stringify(persisted));
	} catch {
		// Chat history cache is best-effort local state.
	}
}

function isPersistableMessage(value: unknown): value is Message {
	if (!value || typeof value !== "object") return false;
	const record = value as Record<string, unknown>;
	return (
		typeof record.id === "number" &&
		(record.role === "user" || record.role === "agent") &&
		typeof record.text === "string" &&
		typeof record.ts === "number"
	);
}
