#!/usr/bin/env bun
/**
 * Headless chat client for poking the nexal backend end-to-end without
 * the web UI. Same wire protocol the browser speaks (`@nexal/transport`
 * `createChatClient`) + Supabase OAuth (PKCE w/ local callback server).
 *
 * Sessions are cached at ~/.nexal/cli-session.json; pass --fresh to
 * force a new login or :logout from the REPL to wipe it.
 *
 * Usage:
 *   bun run src/scripts/chat-cli.ts
 *   bun run src/scripts/chat-cli.ts --url wss://api.nexal.nokiy.net --provider github
 *   bun run src/scripts/chat-cli.ts --once "hello"        # send one and exit
 *   bun run src/scripts/chat-cli.ts --token "<jwt>"       # skip OAuth
 *
 * Inside the REPL:
 *   text         → chat/send
 *   /foo arg     → chat/command (server slash commands)
 *   :quit        → exit
 *   :logout      → clear cached session and exit
 *   :clear       → clear screen
 */
import { createInterface } from "node:readline/promises";
import { stdin as input, stdout as output, exit } from "node:process";
import { chmodSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";

import { createClient, type Session, type SupabaseClient } from "@supabase/supabase-js";

import {
	createChatClient,
	createWebSocketConnection,
	WireErrorMessage,
	type ChatClient,
} from "@nexal/transport";

// ── Config ──────────────────────────────────────────────────────────

const SUPABASE_URL =
	process.env.NEXAL_SUPABASE_URL ??
	process.env.VITE_SUPABASE_URL ??
	"https://oiucjptwjncfbzotwgbg.supabase.co";
// Anon key is safe to ship — it's a public client key (RLS-gated).
const SUPABASE_ANON_KEY =
	process.env.NEXAL_SUPABASE_ANON_KEY ??
	process.env.VITE_SUPABASE_ANON_KEY ??
	"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9pdWNqcHR3am5jZmJ6b3R3Z2JnIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NDc2NTg4MDAsImV4cCI6MjA2MzIzNDgwMH0.bxWvfnWHTLRcXL7UbKKBljxX4Qe8bYgSE4r0FJNHjxk";

const SESSION_PATH = join(homedir(), ".nexal", "cli-session.json");
const CALLBACK_PORT = Number(process.env.NEXAL_CLI_CALLBACK_PORT ?? 8765);
const CALLBACK_URL = `http://127.0.0.1:${CALLBACK_PORT}/callback`;

// ── ANSI helpers ────────────────────────────────────────────────────

const c = {
	dim: (s: string) => `\x1b[2m${s}\x1b[0m`,
	bold: (s: string) => `\x1b[1m${s}\x1b[0m`,
	red: (s: string) => `\x1b[31m${s}\x1b[0m`,
	green: (s: string) => `\x1b[32m${s}\x1b[0m`,
	yellow: (s: string) => `\x1b[33m${s}\x1b[0m`,
	cyan: (s: string) => `\x1b[36m${s}\x1b[0m`,
	gray: (s: string) => `\x1b[90m${s}\x1b[0m`,
};

function logStatus(msg: string) {
	console.log(c.gray(`· ${msg}`));
}
function logError(msg: string) {
	console.log(c.red(`✗ ${msg}`));
}

// ── CLI args ────────────────────────────────────────────────────────

const { values } = parseArgs({
	args: process.argv.slice(2),
	allowPositionals: true,
	options: {
		url: { type: "string" },
		token: { type: "string" },
		provider: { type: "string", default: "github" },
		"chat-id": { type: "string", default: "cli" },
		sender: { type: "string", default: "cli-user" },
		once: { type: "string" },
		fresh: { type: "boolean" },
		help: { type: "boolean", short: "h" },
	},
});

if (values.help) {
	console.log(`nexal-cli — chat REPL for the nexal backend

Usage:
  bun run src/scripts/chat-cli.ts [options]

Options:
  --url <ws-url>         backend WebSocket URL (default $NEXAL_URL or wss://api.nexal.nokiy.net)
  --token <jwt>          skip OAuth, use this Supabase access token directly
  --provider <p>         OAuth provider (default github; google, gitlab, etc. also work)
  --chat-id <id>         chat id sent with messages (default "cli")
  --sender <name>        sender name (default "cli-user")
  --once "<msg>"         send one message, await first reply, exit (non-interactive)
  --fresh                ignore cached session at ~/.nexal/cli-session.json
  -h, --help             this help

Env:
  NEXAL_URL              default backend URL
  NEXAL_TOKEN            default access token (overridden by --token)
  NEXAL_SUPABASE_URL     Supabase project URL
  NEXAL_SUPABASE_ANON_KEY  Supabase anon key
  VITE_SUPABASE_URL / VITE_SUPABASE_ANON_KEY are also accepted

REPL:
  text          → chat/send
  /foo a b      → chat/command (server-side slash command)
  :quit         → exit (Ctrl-D also works)
  :logout       → clear cached session and exit
  :clear        → clear screen
`);
	exit(0);
}

const BACKEND_URL =
	values.url ?? process.env.NEXAL_URL ?? "wss://api.nexal.nokiy.net";

// ── Session cache ───────────────────────────────────────────────────

interface CachedSession {
	access_token: string;
	refresh_token: string;
	expires_at: number;
}

function loadCachedSession(): CachedSession | null {
	try {
		return JSON.parse(readFileSync(SESSION_PATH, "utf8")) as CachedSession;
	} catch {
		return null;
	}
}

function saveSession(session: Session): void {
	mkdirSync(join(homedir(), ".nexal"), { recursive: true });
	const cached: CachedSession = {
		access_token: session.access_token,
		refresh_token: session.refresh_token,
		expires_at: session.expires_at ?? 0,
	};
	writeFileSync(SESSION_PATH, JSON.stringify(cached, null, 2), { mode: 0o600 });
	chmodSync(SESSION_PATH, 0o600);
}

function clearCachedSession(): void {
	try {
		rmSync(SESSION_PATH);
	} catch {
		/* ok */
	}
}

// ── Auth ────────────────────────────────────────────────────────────

function makeSupabase(): SupabaseClient {
	// supabase-js wants storage even with persistSession:false (it uses it
	// for the PKCE code_verifier). Provide an in-memory shim.
	const memory = new Map<string, string>();
	return createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
		auth: {
			flowType: "pkce",
			autoRefreshToken: false,
			persistSession: false,
			detectSessionInUrl: false,
			storage: {
				getItem: (k) => memory.get(k) ?? null,
				setItem: (k, v) => {
					memory.set(k, v);
				},
				removeItem: (k) => {
					memory.delete(k);
				},
			},
		},
	});
}

async function getAccessToken(): Promise<string> {
	if (values.token) return values.token;
	if (process.env.NEXAL_TOKEN) return process.env.NEXAL_TOKEN;

	const supabase = makeSupabase();

	if (!values.fresh) {
		const cached = loadCachedSession();
		if (cached) {
			logStatus("restoring cached session…");
			const { data, error } = await supabase.auth.setSession({
				access_token: cached.access_token,
				refresh_token: cached.refresh_token,
			});
			if (!error && data.session) {
				saveSession(data.session);
				return data.session.access_token;
			}
			logStatus(`cached session expired (${error?.message ?? "unknown"})`);
		}
	}

	return await runOAuthFlow(supabase);
}

async function runOAuthFlow(supabase: SupabaseClient): Promise<string> {
	logStatus(`OAuth via ${values.provider} — opening browser`);

	const { data, error } = await supabase.auth.signInWithOAuth({
		provider: values.provider as "github",
		options: {
			redirectTo: CALLBACK_URL,
			skipBrowserRedirect: true,
		},
	});
	if (error || !data?.url) {
		throw new Error(`signInWithOAuth failed: ${error?.message ?? "no URL"}`);
	}

	const codePromise = waitForCallback();
	openBrowser(data.url);
	console.log(c.dim(`if browser didn't open, visit:\n  ${data.url}\n`));

	const code = await codePromise;

	const { data: session, error: exchangeErr } =
		await supabase.auth.exchangeCodeForSession(code);
	if (exchangeErr || !session.session) {
		throw new Error(`code exchange failed: ${exchangeErr?.message ?? "no session"}`);
	}
	saveSession(session.session);
	logStatus(`signed in as ${session.session.user.email ?? session.session.user.id}`);
	return session.session.access_token;
}

function waitForCallback(): Promise<string> {
	return new Promise<string>((resolve, reject) => {
		const timer = setTimeout(
			() => {
				server.stop();
				reject(new Error("OAuth callback timed out after 5 min"));
			},
			5 * 60 * 1000,
		);

		const server = Bun.serve({
			port: CALLBACK_PORT,
			hostname: "127.0.0.1",
			fetch(req) {
				const url = new URL(req.url);
				if (url.pathname !== "/callback") {
					return new Response("not found", { status: 404 });
				}
				const code = url.searchParams.get("code");
				const oauthErr = url.searchParams.get("error_description");
				if (oauthErr) {
					clearTimeout(timer);
					server.stop();
					reject(new Error(`OAuth error: ${oauthErr}`));
					return new Response(`OAuth error: ${oauthErr}`, { status: 400 });
				}
				if (!code) {
					return new Response(
						"missing ?code — Supabase may have returned the token in the URL " +
							"fragment instead. Pass --token <jwt> manually, or check that " +
							"the OAuth provider is configured for PKCE in the Supabase " +
							"dashboard (Auth → URL Configuration → Redirect URLs must " +
							`include ${CALLBACK_URL}).`,
						{ status: 400 },
					);
				}
				clearTimeout(timer);
				// Defer stop so we can finish responding.
				queueMicrotask(() => server.stop());
				resolve(code);
				return new Response(
					"<!doctype html><meta charset=utf-8><title>nexal CLI</title>" +
						"<style>body{font-family:system-ui;text-align:center;padding:4rem;color:#333}</style>" +
						"<h2>Signed in ✓</h2><p>You can close this tab and return to the terminal.</p>",
					{ headers: { "content-type": "text/html" } },
				);
			},
		});
	});
}

function openBrowser(url: string): void {
	const cmd =
		process.platform === "darwin"
			? ["open", url]
			: process.platform === "win32"
				? ["cmd", "/c", "start", "", url]
				: ["xdg-open", url];
	try {
		const proc = Bun.spawn(cmd, {
			stdin: "ignore",
			stdout: "ignore",
			stderr: "ignore",
		});
		proc.unref();
	} catch {
		/* user can copy URL manually */
	}
}

// ── Chat session ────────────────────────────────────────────────────

async function authenticateIfNeeded(chat: ChatClient): Promise<void> {
	if (!(await serverRequiresAuth(chat))) {
		logStatus("server does not require authentication");
		return;
	}

	let token: string;
	try {
		token = await getAccessToken();
	} catch (err) {
		logError(`auth failed: ${err instanceof Error ? err.message : String(err)}`);
		exit(1);
	}

	try {
		const { userId, email } = await chat.authenticate({ token });
		logStatus(`authenticated as ${email ?? userId}`);
	} catch (err) {
		logError(
			`authenticate failed: ${err instanceof Error ? err.message : String(err)}`,
		);
		exit(1);
	}
}

async function serverRequiresAuth(chat: ChatClient): Promise<boolean> {
	try {
		await chat.listCommands({});
		return false;
	} catch (err) {
		const msg = err instanceof Error ? err.message : String(err);
		if (msg.toLowerCase().includes("not authenticated")) {
			logStatus("server requested authentication");
		} else {
			logStatus(`auth probe failed (${msg}); trying authentication`);
		}
		return true;
	}
}

async function main() {
	logStatus(`connecting to ${BACKEND_URL}`);
	const ws = await createWebSocketConnection(BACKEND_URL, {
		onDisconnect: () => {
			logError("disconnected");
			exit(1);
		},
	});
	const chat = createChatClient(ws.connection);

	await authenticateIfNeeded(chat);

	wireHandlers(chat);

	if (values.once !== undefined) {
		await sendAndWait(chat, values.once);
		ws.connection.close();
		exit(0);
	}

	await repl(chat);
	ws.connection.close();
}

function wireHandlers(chat: ChatClient): void {
	let streamingId: string | null = null;
	let typingShown = false;

	chat.onTyping(() => {
		if (!typingShown) {
			process.stdout.write(c.dim("≈ typing…\r"));
			typingShown = true;
		}
	});
	chat.onReply(({ text }) => {
		if (typingShown) {
			process.stdout.write("\x1b[2K\r");
			typingShown = false;
		}
		console.log(`${c.cyan("←")} ${text}`);
	});
	chat.onReplyChunk(({ messageId, delta }) => {
		if (typingShown) {
			process.stdout.write("\x1b[2K\r");
			typingShown = false;
		}
		if (streamingId !== messageId) {
			if (streamingId !== null) process.stdout.write("\n");
			process.stdout.write(`${c.cyan("←")} `);
			streamingId = messageId;
		}
		process.stdout.write(delta);
	});
	chat.onReplyEnd(({ messageId }) => {
		if (streamingId === messageId) {
			process.stdout.write("\n");
			streamingId = null;
		}
	});
	chat.onCommandResult(({ name, text, error }) => {
		if (error) {
			console.log(`${c.yellow("#")} /${name} ${c.red("error:")} ${error}`);
		} else {
			console.log(`${c.yellow("#")} /${name}: ${text ?? ""}`);
		}
	});
}

async function sendAndWait(chat: ChatClient, message: string): Promise<void> {
	// For `--once`, send and wait until reply_end or a 30s timeout.
	const done = new Promise<void>((resolve) => {
		const t = setTimeout(resolve, 30_000);
		const offEnd = chat.onReplyEnd(() => {
			clearTimeout(t);
			offEnd();
			resolve();
		});
		const offReply = chat.onReply(() => {
			// Non-streaming reply — still close out after one.
			clearTimeout(t);
			offReply();
			resolve();
		});
	});
	dispatchInput(chat, message);
	await done;
}

async function repl(chat: ChatClient): Promise<void> {
	const rl = createInterface({ input, output, prompt: c.bold("> ") });
	console.log(
		c.dim(`connected. type to chat · /cmd for slash commands · :quit to exit`),
	);
	rl.prompt();
	for await (const raw of rl) {
		const line = raw.trim();
		if (!line) {
			rl.prompt();
			continue;
		}
		if (handleLocalCommand(line)) {
			rl.close();
			return;
		}
		dispatchInput(chat, line);
		rl.prompt();
	}
}

function handleLocalCommand(line: string): boolean {
	switch (line) {
		case ":quit":
		case ":exit":
			return true;
		case ":logout":
			clearCachedSession();
			logStatus("cached session removed");
			return true;
		case ":clear":
			process.stdout.write("\x1b[2J\x1b[H");
			return false;
		default:
			return false;
	}
}

function dispatchInput(chat: ChatClient, line: string): void {
	const chatId = values["chat-id"]!;
	const sender = values.sender!;
	if (line.startsWith("/")) {
		const [head, ...args] = line.slice(1).split(/\s+/);
		if (!head) return;
		chat.command({ chatId, sender, name: head, args });
	} else {
		chat.send({ chatId, sender, text: line });
	}
}

main().catch((err) => {
	logError(err instanceof Error ? err.message : String(err));
	exit(1);
});
