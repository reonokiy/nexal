import type { Chat } from "$lib/client.svelte";

export interface Agent {
	agent_id: string;
	container_name: string;
	created_at_unix_ms: number;
}

const CACHE_KEY = "nexal.computers";
const REFRESH_MS = 10_000;

const initialAgents = loadCachedAgents();
let agents = $state<Agent[]>(initialAgents);
let loading = $state(initialAgents.length === 0);
let error = $state<string | null>(null);
let refreshPromise: Promise<void> | null = null;
let interval: ReturnType<typeof setInterval> | null = null;
let subscribers = 0;

function loadCachedAgents(): Agent[] {
	try {
		if (typeof localStorage === "undefined") return [];
		const raw = localStorage.getItem(CACHE_KEY);
		if (!raw) return [];
		const parsed = JSON.parse(raw) as unknown;
		if (!Array.isArray(parsed)) return [];
		return parsed.filter(isAgent);
	} catch {
		return [];
	}
}

function isAgent(value: unknown): value is Agent {
	if (!value || typeof value !== "object") return false;
	const record = value as Record<string, unknown>;
	return (
		typeof record.agent_id === "string" &&
		typeof record.container_name === "string" &&
		typeof record.created_at_unix_ms === "number"
	);
}

function saveCachedAgents(next: Agent[]) {
	try {
		if (typeof localStorage === "undefined") return;
		localStorage.setItem(CACHE_KEY, JSON.stringify(next));
	} catch {
		// Cache writes are opportunistic.
	}
}

export async function refreshComputers(chat: Chat, silent = false): Promise<void> {
	if (refreshPromise) return refreshPromise;
	refreshPromise = doRefreshComputers(chat, silent).finally(() => {
		refreshPromise = null;
	});
	return refreshPromise;
}

async function doRefreshComputers(chat: Chat, silent: boolean): Promise<void> {
	if (chat.status !== "open") {
		if (!silent && agents.length === 0) {
			error = "Backend not connected";
			loading = false;
		}
		return;
	}

	try {
		if (!silent && agents.length === 0) loading = true;
		const res = await chat.runCommandAwait("sandboxes", []);
		if (res.error) throw new Error(res.error);
		const data = res.data as { agents?: Agent[] } | null | undefined;
		agents = data?.agents ?? [];
		saveCachedAgents(agents);
		error = null;
	} catch (e) {
		if (!silent && agents.length === 0) {
			error = e instanceof Error ? e.message : "fetch failed";
		}
	} finally {
		if (!silent) loading = false;
	}
}

export function startComputersRefresh(chat: Chat): () => void {
	subscribers += 1;
	void refreshComputers(chat, agents.length > 0);
	if (!interval) {
		interval = setInterval(() => {
			void refreshComputers(chat, true);
		}, REFRESH_MS);
	}
	return () => {
		subscribers = Math.max(0, subscribers - 1);
		if (subscribers === 0 && interval) {
			clearInterval(interval);
			interval = null;
		}
	};
}

export const computers = {
	get agents() {
		return agents;
	},
	get loading() {
		return loading;
	},
	get error() {
		return error;
	},
};
