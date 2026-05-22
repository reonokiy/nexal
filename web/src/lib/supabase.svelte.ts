/**
 * Supabase Auth client — Svelte 5 runes wrapper.
 *
 * Stores session/token in localStorage so it survives page reloads.
 * The `accessToken` rune is passed to the chat client on connect.
 */
import { createClient } from "@supabase/supabase-js";
import type { Session } from "@supabase/supabase-js";

const SUPABASE_URL = import.meta.env.VITE_SUPABASE_URL ?? "https://oiucjptwjncfbzotwgbg.supabase.co";
const SUPABASE_ANON_KEY = import.meta.env.VITE_SUPABASE_ANON_KEY ?? "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9pdWNqcHR3am5jZmJ6b3R3Z2JnIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NDc2NTg4MDAsImV4cCI6MjA2MzIzNDgwMH0.bxWvfnWHTLRcXL7UbKKBljxX4Qe8bYgSE4r0FJNHjxk";
const PENDING_ROUTE_KEY = "nexal.auth.redirect";
const AUTH_CALLBACK_PATH = "/auth/callback";

export const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
	auth: {
		flowType: "pkce",
		// Handle the callback explicitly on `/auth/callback` to avoid
		// double-processing the PKCE code during client initialisation.
		detectSessionInUrl: false,
		persistSession: true,
		autoRefreshToken: true,
	},
});

// ── Reactive session ────────────────────────────────────────────────

interface StoredSession {
	token: string;
	userId: string;
	email?: string;
	expiresAt: number;
}

function loadStored(): StoredSession | null {
	try {
		const raw = localStorage.getItem("nexal.auth");
		if (!raw) return null;
		const s = JSON.parse(raw) as StoredSession;
		if (s.expiresAt < Date.now() / 1000) {
			localStorage.removeItem("nexal.auth");
			return null;
		}
		return s;
	} catch {
		return null;
	}
}

function saveSession(session: Session | null) {
	if (!session) {
		localStorage.removeItem("nexal.auth");
		return;
	}
	localStorage.setItem(
		"nexal.auth",
		JSON.stringify({
			token: session.access_token,
			userId: session.user.id,
			email: session.user.email,
			expiresAt: session.expires_at ?? 0,
		} satisfies StoredSession),
	);
}

function applySession(session: Session) {
	auth.accessToken = session.access_token;
	auth.userId = session.user.id;
	auth.email = session.user.email ?? "";
	saveSession(session);
}

function clearSession() {
	auth.accessToken = "";
	auth.userId = "";
	auth.email = "";
	saveSession(null);
}

function rememberRouteForAuth() {
	if (typeof window === "undefined") return;
	localStorage.setItem(PENDING_ROUTE_KEY, window.location.hash || "#/");
}

function isAuthCallbackPath() {
	return typeof window !== "undefined" && window.location.pathname === AUTH_CALLBACK_PATH;
}

function hasLegacyAuthHash() {
	return typeof window !== "undefined" && window.location.hash.includes("access_token=");
}

function consumePostAuthUrl(fallbackToHome = false): string | null {
	if (typeof window === "undefined") return null;
	const saved = localStorage.getItem(PENDING_ROUTE_KEY);
	localStorage.removeItem(PENDING_ROUTE_KEY);
	if (!saved && !fallbackToHome) return null;
	const url = new URL(window.location.origin);
	url.pathname = "/";
	url.hash = saved || "#/";
	return url.toString();
}

function redirectAfterAuth(fallbackToHome = false) {
	if (typeof window === "undefined") return;
	const target = consumePostAuthUrl(fallbackToHome);
	if (target && window.location.href !== target) {
		window.location.replace(target);
	}
}

function stripAuthQueryParams() {
	if (typeof window === "undefined") return;
	const url = new URL(window.location.href);
	let changed = false;
	for (const key of ["code", "state", "error", "error_code", "error_description"]) {
		if (!url.searchParams.has(key)) continue;
		url.searchParams.delete(key);
		changed = true;
	}
	if (changed) window.history.replaceState({}, "", url.toString());
}

function formatAuthError(message: string): string {
	if (message.includes("PKCE code verifier not found in storage")) {
		return "Login session expired or was started from a different host. Start again from http://localhost:5173 and keep the whole flow in the same browser tab.";
	}
	return message;
}

// ── Auth state (Svelte 5 runes) ──────────────────────────────────────

let stored = loadStored();

class AuthState {
	accessToken = $state(stored?.token ?? "");
	userId = $state(stored?.userId ?? "");
	email = $state(stored?.email ?? "");
	initialised = $state(false);
	error = $state("");
}

export const auth = new AuthState();

export function getAccessToken(): string {
	return auth.accessToken;
}

export function isLoggedIn(): boolean {
	return auth.accessToken.length > 0;
}

export function getUserId(): string {
	return auth.userId;
}

// ── Actions ─────────────────────────────────────────────────────────

export async function signIn(email: string, password: string): Promise<{ error?: string }> {
	const { data, error } = await supabase.auth.signInWithPassword({ email, password });
	if (error) return { error: error.message };
	applySession(data.session);
	return {};
}

export async function signUp(email: string, password: string): Promise<{ error?: string }> {
	const { data, error } = await supabase.auth.signUp({ email, password });
	if (error) return { error: error.message };
	// Supabase requires email confirmation by default — user won't be fully signed in.
	return { error: data.session ? undefined : "Check your email for a confirmation link." };
}

export async function signOut(): Promise<void> {
	await supabase.auth.signOut();
	clearSession();
}

export async function signInWithProvider(
	provider: "google" | "github" | "azure" | "gitlab" | "bitbucket" | "discord",
): Promise<{ error?: string }> {
	rememberRouteForAuth();
	auth.error = "";
	const { error } = await (supabase.auth as any).signInWithOAuth({
		provider,
		options: {
			redirectTo: `${window.location.origin}${AUTH_CALLBACK_PATH}`,
		},
	});
	if (error) return { error: formatAuthError(error.message) };
	return {};
}

// ── Session recovery on load ────────────────────────────────────────

supabase.auth.onAuthStateChange((_event, session) => {
	if (session) {
		applySession(session);
	} else {
		clearSession();
	}
});

// Force Supabase to parse the OAuth callback hash (or restore stored session)
// and update our reactive state. This avoids races where the URL hash is
// stripped before the implicit flow finishes.
async function bootstrapSession() {
	auth.initialised = false;
	auth.error = "";
	try {
		const { data } = await supabase.auth.getSession();
		if (data.session) {
			applySession(data.session);
			stripAuthQueryParams();
			if (isAuthCallbackPath()) {
				redirectAfterAuth(true);
				return;
			}
			return;
		}

		// PKCE redirect fallback: if Supabase did not auto-exchange the `?code=...`
		// callback yet, do it explicitly.
		if (typeof window !== "undefined") {
			const url = new URL(window.location.href);
			const code = url.searchParams.get("code");
			if (code) {
				const { data: exchangeData, error } = await (supabase.auth as any).exchangeCodeForSession(code);
				if (error) {
					auth.error = formatAuthError(error.message);
				} else if (exchangeData.session) {
					applySession(exchangeData.session);
					stripAuthQueryParams();
					redirectAfterAuth(true);
					return;
				}
			}
		}

		// Fallback: if the URL still has an OAuth implicit-flow hash but Supabase
		// didn't pick it up (e.g. flowType mismatch), set it manually.
		if (typeof window !== "undefined" && window.location.hash.includes("access_token=")) {
			const params = new URLSearchParams(window.location.hash.slice(1));
			const access_token = params.get("access_token");
			const refresh_token = params.get("refresh_token") ?? "";
			if (access_token) {
				const { data: setData, error } = await supabase.auth.setSession({
					access_token,
					refresh_token,
				});
					if (!error && setData.session) {
						applySession(setData.session);
						redirectAfterAuth(true);
						return;
					}
					if (error) auth.error = formatAuthError(error.message);
				}
			}

		if (isAuthCallbackPath()) {
			stripAuthQueryParams();
		}
	} finally {
		auth.initialised = true;
	}
}
bootstrapSession();
