import type { TapeStore } from "../tape/store.ts";

export const mockTapeStore: TapeStore = {
	listTapes: async () => [],
	read: async () => [],
	append: async () => {},
	reset: async () => {},
	info: async () => ({
		name: "",
		entries: 0,
		anchors: 0,
		lastAnchor: null,
		entriesSinceLastAnchor: 0,
		lastTokenUsage: null,
	}),
	handoff: async () => {},
	search: async () => [],
};
