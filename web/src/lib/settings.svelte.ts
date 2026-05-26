/**
 * App-wide settings backed by localStorage.
 *
 * Each setter writes through to localStorage and (where applicable)
 * applies the side effect — e.g. toggling the `dark` class on
 * `<html>` when the theme changes.
 *
 * Keep the API additive: settings consumers read via getters and
 * mutate via setters; the store is reactive thanks to Svelte 5 runes.
 */

export type Theme = "light" | "dark" | "system";
export type SendKey = "enter" | "shift-enter";

const PREFIX = "nexal.";
const CANONICAL_BACKEND_URL = "wss://api.nexal.nokiy.net";
const LEGACY_BACKEND_URLS = new Set(["wss://nexal-server.fly.dev"]);
const DEFAULT_BACKEND_URL = normalizeBackendUrl(
	import.meta.env.VITE_NEXAL_BACKEND ?? CANONICAL_BACKEND_URL,
);

function normalizeBackendUrl(value: string): string {
	return LEGACY_BACKEND_URLS.has(value) ? CANONICAL_BACKEND_URL : value;
}

function read<T>(key: string, fallback: T): T {
	if (typeof localStorage === "undefined") return fallback;
	const raw = localStorage.getItem(PREFIX + key);
	if (raw === null) return fallback;
	try {
		return JSON.parse(raw) as T;
	} catch {
		return fallback;
	}
}

function write(key: string, value: unknown): void {
	if (typeof localStorage === "undefined") return;
	localStorage.setItem(PREFIX + key, JSON.stringify(value));
}

export function applyTheme(theme: Theme): void {
	if (typeof document === "undefined") return;
	const cls = document.documentElement.classList;
	let dark: boolean;
	if (theme === "dark") dark = true;
	else if (theme === "light") dark = false;
	else dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
	cls.toggle("dark", dark);
}

class SettingsStore {
	#theme = $state<Theme>(read<Theme>("theme", "system"));
	#sendKey = $state<SendKey>(read<SendKey>("sendKey", "enter"));
	#autoReconnect = $state(read<boolean>("autoReconnect", true));
	#backendUrl = $state(
		normalizeBackendUrl(read<string>("backendUrl", DEFAULT_BACKEND_URL)),
	);
	#chatId = $state(read<string>("chatId", "default"));
	#sender = $state(read<string>("sender", "web-user"));
	#showTimestamps = $state(read<boolean>("showTimestamps", true));
	#showSystem = $state(read<boolean>("showSystem", true));
	#compact = $state(read<boolean>("compact", false));

	get theme() {
		return this.#theme;
	}
	set theme(v: Theme) {
		this.#theme = v;
		write("theme", v);
		applyTheme(v);
	}

	get sendKey() {
		return this.#sendKey;
	}
	set sendKey(v: SendKey) {
		this.#sendKey = v;
		write("sendKey", v);
	}

	get autoReconnect() {
		return this.#autoReconnect;
	}
	set autoReconnect(v: boolean) {
		this.#autoReconnect = v;
		write("autoReconnect", v);
	}

	get backendUrl() {
		return this.#backendUrl;
	}
	set backendUrl(v: string) {
		this.#backendUrl = v;
		write("backendUrl", v);
	}

	get chatId() {
		return this.#chatId;
	}
	set chatId(v: string) {
		this.#chatId = v;
		write("chatId", v);
	}

	get sender() {
		return this.#sender;
	}
	set sender(v: string) {
		this.#sender = v;
		write("sender", v);
	}

	get showTimestamps() {
		return this.#showTimestamps;
	}
	set showTimestamps(v: boolean) {
		this.#showTimestamps = v;
		write("showTimestamps", v);
	}

	get showSystem() {
		return this.#showSystem;
	}
	set showSystem(v: boolean) {
		this.#showSystem = v;
		write("showSystem", v);
	}

	get compact() {
		return this.#compact;
	}
	set compact(v: boolean) {
		this.#compact = v;
		write("compact", v);
	}

	resetAll(): void {
		this.theme = "system";
		this.sendKey = "enter";
		this.autoReconnect = true;
		this.backendUrl = DEFAULT_BACKEND_URL;
		this.chatId = "default";
		this.sender = "web-user";
		this.showTimestamps = true;
		this.showSystem = true;
		this.compact = false;
	}
}

export const settings = new SettingsStore();

if (typeof window !== "undefined") {
	// Re-evaluate "system" theme when the OS preference changes.
	window
		.matchMedia("(prefers-color-scheme: dark)")
		.addEventListener("change", () => {
			if (settings.theme === "system") applyTheme("system");
		});
}
