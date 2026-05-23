import type { AgentMessage } from "@mariozechner/pi-agent-core";
import type { ContextStore, ContextLoadOptions } from "./types.ts";
import type { TapeStore } from "../tape/store.ts";
import type { WorkerStore } from "../workers/store.ts";
import { entriesToMessages, messagesToEntries, messagesToJson, jsonToMessages } from "./converter.ts";

const DEFAULT_MAX_MESSAGES = 200;

export function createContextStore(opts: {
	tapeStore: TapeStore;
	workerStore?: WorkerStore;
}): ContextStore {
	const { tapeStore, workerStore } = opts;

	return {
		async load(sessionKey: string, loadOpts?: ContextLoadOptions): Promise<AgentMessage[]> {
			const maxMessages = loadOpts?.maxMessages ?? DEFAULT_MAX_MESSAGES;

			// Try tape first.
			try {
				const entries = await tapeStore.read(sessionKey);
				if (entries.length > 0) {
					const messages = entriesToMessages(entries);
					return messages.length > maxMessages ? messages.slice(-maxMessages) : messages;
				}
			} catch {
				// Fall through to DB.
			}

			// Fallback: try DB messages_json (worker-style session key "worker:<id>").
			if (workerStore && sessionKey.startsWith("worker:")) {
				const workerId = sessionKey.slice("worker:".length);
				try {
					const row = await workerStore.get(workerId);
					if (row?.messagesJson) {
						const messages = jsonToMessages(row.messagesJson);
						return messages.length > maxMessages ? messages.slice(-maxMessages) : messages;
					}
				} catch {
					// No data available.
				}
			}

			return [];
		},

		async save(sessionKey: string, messages: AgentMessage[]): Promise<void> {
			// Write to tape as entries.
			const entries = messagesToEntries(messages);
			await tapeStore.reset(sessionKey);
			for (const entry of entries) {
				await tapeStore.append(sessionKey, {
					...entry,
					date: new Date().toISOString(),
				});
			}

			// Also update DB messages_json for worker sessions.
			if (workerStore && sessionKey.startsWith("worker:")) {
				const workerId = sessionKey.slice("worker:".length);
				try {
					await workerStore.setMessages(workerId, messagesToJson(messages), 0);
				} catch {
					// Best-effort.
				}
			}
		},

		async appendDelta(sessionKey: string, fromIndex: number, messages: AgentMessage[]): Promise<void> {
			const newMessages = messages.slice(fromIndex);
			if (newMessages.length === 0) return;

			const entries = messagesToEntries(newMessages);
			for (const entry of entries) {
				await tapeStore.append(sessionKey, {
					...entry,
					date: new Date().toISOString(),
				});
			}
		},
	};
}
