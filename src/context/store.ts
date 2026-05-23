import type { TapeStore } from "../tape/store.ts";
import type { MemoryStore } from "./types.ts";

export function createMemoryStore(tapeStore: TapeStore): MemoryStore {
	return {
		async load(sessionKey) {
			return tapeStore.read(sessionKey);
		},
		async append(sessionKey, entries) {
			for (const entry of entries) {
				await tapeStore.append(sessionKey, {
					...entry,
					date: entry.date ?? new Date().toISOString(),
				});
			}
		},
		async replace(sessionKey, entries) {
			await tapeStore.reset(sessionKey);
			for (const entry of entries) {
				await tapeStore.append(sessionKey, {
					...entry,
					date: entry.date ?? new Date().toISOString(),
				});
			}
		},
	};
}
