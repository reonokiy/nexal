/**
 * Nexal TUI — terminal chat client that connects to the WsChannel
 * over a Unix domain socket (default) or TCP WebSocket.
 *
 * Usage:
 *   bun run src/tui.ts                          # Unix socket ~/.nexal/nexal.sock
 *   bun run src/tui.ts --unix /tmp/nexal.sock   # Custom Unix socket
 *   bun run src/tui.ts --port 3001              # TCP mode
 *   bun run src/tui.ts --chat-id myChat         # Custom chat ID
 */
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";
import WS from "ws";
import chalk from "chalk";
import { saveAuth, saveModelConfig, loadAuth, loadModelConfig } from "./settings.ts";
import {
	TUI,
	ProcessTerminal,
	Editor,
	Markdown,
	Text,
	Loader,
	Container,
	Spacer,
	CombinedAutocompleteProvider,
	type MarkdownTheme,
	type EditorTheme,
	type SelectListTheme,
	matchesKey,
	Key,
} from "@mariozechner/pi-tui";

// ── CLI args ────────────────────────────────────────────────────────

function parseArgs(argv: string[]): {
	unix: string | null;
	host: string;
	port: number;
	chatId: string;
} {
	let unix: string | null = join(homedir(), ".nexal", "nexal.sock");
	let host = "127.0.0.1";
	let port = 0;
	let chatId = "tui";

	for (let i = 2; i < argv.length; i++) {
		const arg = argv[i];
		const next = argv[i + 1];
		if (arg === "--unix" && next) {
			unix = next;
			i++;
		} else if (arg === "--host" && next) {
			host = next;
			i++;
		} else if (arg === "--port" && next) {
			port = Number(next);
			unix = null; // TCP mode
			i++;
		} else if (arg === "--chat-id" && next) {
			chatId = next;
			i++;
		}
	}
	return { unix, host, port, chatId };
}

const args = parseArgs(process.argv);

// ── Theme ───────────────────────────────────────────────────────────

const markdownTheme: MarkdownTheme = {
	heading: (s) => chalk.bold.cyan(s),
	link: (s) => chalk.blue(s),
	linkUrl: (s) => chalk.dim(s),
	code: (s) => chalk.yellow(s),
	codeBlock: (s) => chalk.green(s),
	codeBlockBorder: (s) => chalk.dim(s),
	quote: (s) => chalk.italic(s),
	quoteBorder: (s) => chalk.dim(s),
	hr: (s) => chalk.dim(s),
	listBullet: (s) => chalk.cyan(s),
	bold: (s) => chalk.bold(s),
	italic: (s) => chalk.italic(s),
	strikethrough: (s) => chalk.strikethrough(s),
	underline: (s) => chalk.underline(s),
};

const selectListTheme: SelectListTheme = {
	selectedPrefix: (s) => chalk.blue(s),
	selectedText: (s) => chalk.bold(s),
	description: (s) => chalk.dim(s),
	scrollInfo: (s) => chalk.dim(s),
	noMatch: (s) => chalk.dim(s),
};

const editorTheme: EditorTheme = {
	borderColor: (s) => chalk.gray(s),
	selectList: selectListTheme,
};

// ── TUI setup ───────────────────────────────────────────────────────

const terminal = new ProcessTerminal();
const tui = new TUI(terminal);

const history = new Container();
tui.addChild(history);

const editor = new Editor(tui, editorTheme);
editor.setAutocompleteProvider(
	new CombinedAutocompleteProvider(
		[
			{ name: "login", description: "OAuth login (e.g. /login anthropic)" },
			{ name: "model", description: "Set model (e.g. /model anthropic claude-sonnet-4-6)" },
			{ name: "clear", description: "Clear chat history" },
			{ name: "help", description: "Show available commands" },
			{ name: "quit", description: "Exit TUI" },
		],
		process.cwd(),
	),
);
{
	const origHandleInput = editor.handleInput.bind(editor);
	editor.handleInput = (data: string) => {
		if (matchesKey(data, Key.escape) && waiting) {
			hideLoader();
			addSystemNote("Cancelled");
			finishReply();
			return;
		}
		origHandleInput(data);
	};
}
tui.addChild(editor);
tui.setFocus(editor);

let waiting = false;
let loader: Loader | null = null;

function addUserMessage(text: string): void {
	history.addChild(new Spacer(1));
	history.addChild(new Text(chalk.bold.green("You"), 1, 0));
	history.addChild(new Markdown(text, 1, 0, markdownTheme));
}

function addBotReply(text: string): void {
	history.addChild(new Spacer(1));
	history.addChild(new Text(chalk.bold.magenta("Nexal"), 1, 0));
	history.addChild(new Markdown(text, 1, 0, markdownTheme));
}

function addSystemNote(text: string): void {
	history.addChild(new Text(chalk.dim(`--- ${text} ---`), 1, 0));
}

function showLoader(): void {
	if (loader) return;
	loader = new Loader(
		tui,
		(s) => chalk.cyan(s),
		(s) => chalk.gray(s),
		"Thinking...",
	);
	history.addChild(loader);
	loader.start();
	tui.requestRender();
}

function hideLoader(): void {
	if (!loader) return;
	loader.stop();
	history.removeChild(loader);
	loader = null;
}

function finishReply(): void {
	waiting = false;
	editor.disableSubmit = false;
	tui.setFocus(editor);
	tui.requestRender();
}

// ── WebSocket ───────────────────────────────────────────────────────

let ws: WS | null = null;

function createWs(): WS {
	if (args.unix) {
		// Connect via Unix domain socket using a raw TCP socket.
		return new WS("ws://localhost/ws", {
			createConnection: () => createConnection(args.unix!),
		});
	}
	return new WS(`ws://${args.host}:${args.port}/ws`);
}

function connect(): void {
	ws = createWs();

	ws.on("open", () => {
		addSystemNote("Connected");
		tui.requestRender();
	});

	ws.on("message", (raw: WS.RawData) => {
		const text = typeof raw === "string" ? raw : raw.toString("utf-8");
		let msg: { type?: string; chat_id?: string; text?: string };
		try {
			msg = JSON.parse(text);
		} catch {
			return;
		}

		if (msg.type === "reply" && typeof msg.text === "string") {
			hideLoader();
			addBotReply(msg.text);
			finishReply();
		} else if (msg.type === "typing") {
			showLoader();
		}
	});

	ws.on("close", () => {
		hideLoader();
		if (waiting) finishReply();
		addSystemNote("Disconnected — reconnecting...");
		tui.requestRender();
		ws = null;
		setTimeout(connect, 2_000);
	});

	ws.on("error", () => {
		// `close` will fire next — reconnect happens there.
	});
}

// ── Input handling ──────────────────────────────────────────────────

editor.onSubmit = (text: string) => {
	const trimmed = text.trim();
	if (!trimmed) return;

	if (trimmed === "/quit" || trimmed === "/exit") {
		shutdown();
		return;
	}

	if (trimmed === "/clear") {
		history.clear();
		editor.setText("");
		tui.requestRender();
		return;
	}

	if (waiting) return;

	if (trimmed.startsWith("/login")) {
		editor.setText("");
		void handleLogin(trimmed);
		return;
	}

	if (trimmed.startsWith("/model")) {
		editor.setText("");
		void handleModel(trimmed);
		return;
	}

	if (trimmed === "/help") {
		editor.setText("");
		addSystemNote("Commands: /login <provider>, /model <provider> <model>, /clear, /quit");
		tui.requestRender();
		return;
	}

	waiting = true;
	editor.disableSubmit = true;
	editor.setText("");
	addUserMessage(trimmed);
	showLoader();
	tui.requestRender();

	if (ws && ws.readyState === WS.OPEN) {
		ws.send(
			JSON.stringify({
				type: "send",
				chat_id: args.chatId,
				sender: "tui-user",
				text: trimmed,
			}),
		);
	}
};

// ── Slash commands ──────────────────────────────────────────────────

async function handleLogin(input: string): Promise<void> {
	const parts = input.split(/\s+/);
	const provider = parts[1] ?? "anthropic";

	addSystemNote(`Logging in to ${provider}...`);
	tui.requestRender();

	try {
		const { getOAuthProvider } = await import("@mariozechner/pi-ai/oauth");
		const oauthProvider = getOAuthProvider(provider);
		if (!oauthProvider) {
			addSystemNote(`Unknown OAuth provider: ${provider}. Try: anthropic`);
			tui.requestRender();
			return;
		}

		// Temporarily stop TUI so the OAuth callback server can use stdin if needed,
		// and so the "open browser" message is visible.
		tui.stop();

		const creds = await oauthProvider.login({
			onAuth: (info) => {
				console.log(`\nOpen this URL to authorize:\n  ${info.url}\n`);
				if (info.instructions) console.log(info.instructions);
				// Try to open browser automatically.
				const cmd = process.platform === "darwin" ? "open" : "xdg-open";
				Bun.spawn([cmd, info.url], { stdout: "ignore", stderr: "ignore" });
			},
			onPrompt: async (prompt) => {
				// Fallback manual input if callback server doesn't work.
				const rl = await import("node:readline");
				const iface = rl.createInterface({ input: process.stdin, output: process.stdout });
				return new Promise<string>((resolve) => {
					iface.question(`${prompt.message}: `, (answer) => {
						iface.close();
						resolve(answer);
					});
				});
			},
			onProgress: (msg) => {
				console.log(`  ${msg}`);
			},
		});

		// Save credentials.
		await saveAuth({
			provider,
			type: "oauth",
			access: creds.access,
			refresh: creds.refresh,
			expires: creds.expires,
		});

		// Also save as default model config if not already set.
		const existing = await loadModelConfig();
		if (!existing) {
			const defaultModel = provider === "anthropic" ? "claude-sonnet-4-6" : "";
			if (defaultModel) await saveModelConfig(provider, defaultModel);
		}

		// Restart TUI.
		tui.start();
		addSystemNote(`Logged in to ${provider} successfully! Restart nexal to use.`);
		tui.requestRender();
	} catch (err: any) {
		// Restart TUI even on error.
		tui.start();
		addSystemNote(`Login failed: ${err?.message ?? err}`);
		tui.requestRender();
	}
}

async function handleModel(input: string): Promise<void> {
	const parts = input.split(/\s+/);
	const provider = parts[1];
	const modelId = parts[2];

	if (!provider || !modelId) {
		const saved = await loadModelConfig();
		if (saved) {
			addSystemNote(`Current model: ${saved.provider} / ${saved.modelId}`);
		} else {
			addSystemNote("Usage: /model <provider> <model_id>");
			addSystemNote("Example: /model anthropic claude-sonnet-4-6");
		}
		tui.requestRender();
		return;
	}

	await saveModelConfig(provider, modelId);
	addSystemNote(`Model set to ${provider} / ${modelId}. Restart nexal to apply.`);
	tui.requestRender();
}

// ── Lifecycle ───────────────────────────────────────────────────────

function shutdown(): void {
	hideLoader();
	ws?.close();
	tui.stop();
	process.exit(0);
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

// Go
addSystemNote(`nexal-tui  (chat_id=${args.chatId})`);
addSystemNote(
	args.unix
		? `Connecting to unix:${args.unix}`
		: `Connecting to ws://${args.host}:${args.port}`,
);
tui.start();
connect();
