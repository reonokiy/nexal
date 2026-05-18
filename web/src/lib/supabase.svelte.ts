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

export const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY);

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

// ── Auth state (Svelte 5 runes) ──────────────────────────────────────

let stored = loadStored();

let accessToken = $state(stored?.token ?? "");
let currentUserId = $state(stored?.userId ?? "");
let currentEmail = $state(stored?.email ?? "");

export function getAccessToken(): string {
	return accessToken;
}

export function isLoggedIn(): boolean {
	return accessToken.length > 0;
}

export function getUserId(): string {
	return currentUserId;
}

// ── Actions ─────────────────────────────────────────────────────────

export async function signIn(email: string, password: string): Promise<{ error?: string }> {
	const { data, error } = await supabase.auth.signInWithPassword({ email, password });
	if (error) return { error: error.message };
	accessToken = data.session.access_token;
	currentUserId = data.session.user.id;
	currentEmail = data.session.user.email ?? "";
	saveSession(data.session);
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
	accessToken = "";
	currentUserId = "";
	currentEmail = "";
	saveSession(null);
}

// ── Session recovery on load ────────────────────────────────────────

supabase.auth.onAuthStateChange((_event, session) => {
	if (session) {
		accessToken = session.access_token;
		currentUserId = session.user.id;
		currentEmail = session.user.email ?? "";
		saveSession(session);
	}
});

// Recover from stored session on load
if (stored?.token) {
	supabase.auth.setSession({
		access_token: stored.token,
		refresh_token: "",
	}).catch(() => {
		saveSession(null);
		accessToken = "";
		currentUserId = "";
		currentEmail = "";
	});
}
