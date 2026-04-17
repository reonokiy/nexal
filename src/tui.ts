/**
 * nexal TUI — terminal chat client that connects to the WsChannel
 * over a Unix domain socket (default) or TCP WebSocket.
 */
import { parseArgs } from "node:util";
import WS from "ws";
import chalk from "chalk";
import { saveAuth, saveModelConfig, loadAuth, loadModelConfig } from "./settings.ts";
import type {
	WsServerFrame,
	WsSendFrame,
	WsCommandFrame,
	WsImageBlock,
} from "./channels/ws-protocol.ts";
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
import { readClipboardImage } from "./clipboard.ts";

// ── CLI args ────────────────────────────────────────────────────────

// When launched via `nexal -i`, args come through env vars.
// When launched directly, parseArgs handles them.
const isEmbedded = !!process.env.NEXAL_TUI_EMBEDDED;
const { values: cli } = isEmbedded
	? { values: {} as Record<string, any> }
	: parseArgs({
		options: {
			host:    { type: "string", default: "127.0.0.1" },
			port:    { type: "string", short: "p", default: "3001" },
			"chat-id": { type: "string", default: "tui" },
			help:    { type: "boolean", short: "h" },
		},
		strict: true,
		allowPositionals: false,
	});

if (cli.help) {
	console.log(`nexal-tui — terminal chat client

Usage: nexal-tui [options]

Options:
      --host <addr>       WebSocket host (default: 127.0.0.1)
  -p, --port <number>     WebSocket port (default: 3001)
      --chat-id <id>      Chat session ID (default: tui)
  -h, --help              Show this help`);
	process.exit(0);
}

const args = {
	host: process.env.NEXAL_TUI_HOST ?? cli.host ?? "127.0.0.1",
	port: Number(process.env.NEXAL_TUI_PORT ?? cli.port ?? "3001"),
	chatId: process.env.NEXAL_TUI_CHAT_ID ?? cli["chat-id"] ?? "tui",
};

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
{
	const PROMPT_PREFIX = "> ";
	const PROMPT_WIDTH = 2;
	const origRender = editor.render.bind(editor);
	editor.render = (width: number): string[] => {
		// Render editor in a narrower width, then prepend "> " to content lines.
		const lines = origRender(width - PROMPT_WIDTH);
		// lines[0] = top border, lines[last] = bottom border, middle = content.
		if (lines.length > 2) {
			// Extend borders to full width.
			lines[0] = chalk.gray("─".repeat(PROMPT_WIDTH)) + lines[0];
			lines[lines.length - 1] = chalk.gray("─".repeat(PROMPT_WIDTH)) + lines[lines.length - 1];
			// Prepend prompt to content lines.
			for (let i = 1; i < lines.length - 1; i++) {
				lines[i] = chalk.bold.green(PROMPT_PREFIX) + lines[i];
			}
		}
		return lines;
	};
}
editor.setAutocompleteProvider(
	new CombinedAutocompleteProvider(
		[
			{ name: "login", description: "OAuth login (e.g. /login anthropic)" },
			{ name: "model", description: "Set model (e.g. /model anthropic claude-sonnet-4-6)" },
			{ name: "status", description: "Show nexal system status" },
			{ name: "help", description: "Show available commands" },
			{ name: "clear", description: "Clear chat history" },
			{ name: "quit", description: "Exit TUI" },
		],
		process.cwd(),
	),
);
{
	const origHandleInput = editor.handleInput.bind(editor);
	editor.handleInput = (data: string) => {
		if (matchesKey(data, Key.ctrl("c"))) {
			if (ctrlCPending) {
				shutdown();
				return;
			}
			ctrlCPending = true;
			setStatus("Press Ctrl+C again to exit");
			if (ctrlCTimer) clearTimeout(ctrlCTimer);
			ctrlCTimer = setTimeout(() => {
				ctrlCPending = false;
				setStatus(`nexal-tui  chat_id=${args.chatId}  ws://${args.host}:${args.port}  ●`);
			}, 2_000);
			return;
		}
		ctrlCPending = false;
		if (ctrlCTimer) { clearTimeout(ctrlCTimer); ctrlCTimer = null; }
		if (matchesKey(data, Key.escape) && waiting) {
			hideLoader();
			addSystemNote("Cancelled");
			finishReply();
			return;
		}
		// Ctrl+V — check clipboard for image before normal paste.
		if (matchesKey(data, Key.ctrl("v"))) {
			handleClipboardPaste().then((found) => {
				if (!found) {
					// No image — forward to editor for normal text handling.
					origHandleInput(data);
				}
			});
			return;
		}
		origHandleInput(data);
	};
}
tui.addChild(editor);

const statusLine = new Text(chalk.dim(""), 0, 0);
tui.addChild(statusLine);

tui.setFocus(editor);

let waiting = false;
let loader: Loader | null = null;
let ctrlCPending = false;
let ctrlCTimer: ReturnType<typeof setTimeout> | null = null;

// ── Clipboard image paste state ────────────────────────────────────

/** Pending images attached via Ctrl+V, sent with the next message. */
const pendingImages: WsImageBlock[] = [];

function updateImageIndicator(): void {
	if (pendingImages.length > 0) {
		setStatus(
			`nexal-tui  chat_id=${args.chatId}  ` +
			chalk.yellow(`📎 ${pendingImages.length} image(s)`) +
			`  ws://${args.host}:${args.port}  ●`,
		);
	}
}

async function handleClipboardPaste(): Promise<boolean> {
	const png = await readClipboardImage();
	if (!png) return false;

	const b64 = Buffer.from(png).toString("base64");
	pendingImages.push({ data: b64, mimeType: "image/png" });
	editor.insertTextAtCursor(`[image ${pendingImages.length}]`);
	updateImageIndicator();
	tui.requestRender();
	return true;
}

function setStatus(text: string): void {
	statusLine.text = chalk.dim(text);
	tui.requestRender();
}

// ── Tree-style chat rendering ──────────────────────────────────────

let inNexalGroup = false;
let currentWorkerName: string | null = null;
let workerMsgCount = 0;
let lastBranch: { widget: Text; sealedText: string } | null = null;

function sealBranch(): void {
	if (!lastBranch) return;
	lastBranch.widget.text = lastBranch.sealedText;
	history.addChild(new Text(chalk.dim("│"), 0, 0));
	lastBranch = null;
	currentWorkerName = null;
	workerMsgCount = 0;
}

function closeNexalGroup(): void {
	lastBranch = null;
	currentWorkerName = null;
	workerMsgCount = 0;
	inNexalGroup = false;
}

function ensureNexalHeader(): void {
	if (!inNexalGroup) {
		history.addChild(new Spacer(1));
		history.addChild(new Text(chalk.bold.magenta("nexal") + chalk.dim(" (coordinator)"), 0, 0));
		inNexalGroup = true;
	}
}

function addUserMessage(text: string): void {
	closeNexalGroup();
	history.addChild(new Spacer(1));
	history.addChild(new Text(chalk.bold.green("user"), 0, 0));
	history.addChild(new Text(chalk.dim("└ ") + text, 0, 0));
}

interface WorkerMeta {
	name?: string;
	kind?: string;
	lifetime?: string;
}

function addBotReply(text: string, worker?: WorkerMeta): void {
	if (worker?.name) {
		const kind = worker.kind ?? "worker";
		const lifetime = worker.lifetime ?? "";
		const tag = lifetime ? `${kind} · ${lifetime}` : kind;
		const label = chalk.bold.cyan(worker.name) + chalk.dim(` (${tag})`);

		if (worker.name === currentWorkerName) {
			// Same worker — separate with a dim line
			workerMsgCount++;
			history.addChild(new Text(chalk.dim("   ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌"), 0, 0));
			history.addChild(new Markdown(text, 3, 0, markdownTheme));
			return;
		}

		// Different worker or first worker
		sealBranch();
		ensureNexalHeader();

		const widget = new Text(chalk.dim("└─ ") + label, 0, 0);
		history.addChild(widget);
		lastBranch = {
			widget,
			sealedText: chalk.dim("├─ ") + label,
		};
		currentWorkerName = worker.name;
		workerMsgCount = 1;
		history.addChild(new Markdown(text, 3, 0, markdownTheme));
	} else {
		// Coordinator direct message — use Markdown for rich formatting.
		sealBranch();
		ensureNexalHeader();

		history.addChild(new Markdown(text, 1, 0, markdownTheme));
	}
}

function addSystemNote(text: string): void {
	history.addChild(new Text(chalk.dim(text), 1, 0));
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
	return new WS(`ws://${args.host}:${args.port}/ws`);
}

function connect(): void {
	ws = createWs();

	ws.on("open", () => {
		setStatus(`nexal-tui  chat_id=${args.chatId}  ws://${args.host}:${args.port}  ●`);
	});

	ws.on("message", (raw: WS.RawData) => {
		const text = typeof raw === "string" ? raw : raw.toString("utf-8");
		let frame: WsServerFrame;
		try {
			frame = JSON.parse(text);
		} catch {
			return;
		}

		if (frame.type === "reply") {
			hideLoader();
			addBotReply(frame.text, frame.metadata?.worker);
			finishReply();
		} else if (frame.type === "command_result") {
			hideLoader();
			addSystemNote(frame.error ?? frame.text ?? "");
			finishReply();
		} else if (frame.type === "typing") {
			showLoader();
		}
	});

	ws.on("close", () => {
		hideLoader();
		if (waiting) finishReply();
		setStatus(`nexal-tui  chat_id=${args.chatId}  ws://${args.host}:${args.port}  ○ reconnecting...`);
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
		closeNexalGroup();
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

	// Slash commands → send as structured command message.
	if (trimmed.startsWith("/")) {
		const parts = trimmed.slice(1).split(/\s+/);
		const cmdName = parts[0]!;
		const cmdArgs = parts.slice(1);
		waiting = true;
		editor.disableSubmit = true;
		editor.addToHistory(trimmed);
		editor.setText("");
		showLoader();
		tui.requestRender();
		if (ws && ws.readyState === WS.OPEN) {
			const frame: WsCommandFrame = {
				type: "command",
				chat_id: args.chatId,
				sender: "tui-user",
				name: cmdName,
				args: cmdArgs,
			};
			ws.send(JSON.stringify(frame));
		}
		return;
	}

	if (!ws || ws.readyState !== WS.OPEN) {
		addSystemNote("Not connected — message not sent.");
		tui.requestRender();
		return;
	}

	waiting = true;
	editor.disableSubmit = true;
	editor.addToHistory(trimmed);
	editor.setText("");

	const hasImages = pendingImages.length > 0;
	const label = hasImages ? `${trimmed} (+ ${pendingImages.length} image)` : trimmed;
	addUserMessage(label);
	showLoader();
	tui.requestRender();

	const frame: WsSendFrame = {
		type: "send",
		chat_id: args.chatId,
		sender: "tui-user",
		text: trimmed,
		...(hasImages && { images: [...pendingImages] }),
	};
	pendingImages.length = 0;
	setStatus(`nexal-tui  chat_id=${args.chatId}  ws://${args.host}:${args.port}  ●`);
	ws.send(JSON.stringify(frame));
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
setStatus(`nexal-tui  chat_id=${args.chatId}  ws://${args.host}:${args.port}  ○`);
tui.start();
connect();
