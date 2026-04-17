#!/usr/bin/env bun
/**
 * nexal unified CLI entry point.
 *
 *   nexal           → start the daemon (gateway + channels + agent pool)
 *   nexal -i        → launch interactive TUI client
 *   nexal --help    → show help
 */
import { parseArgs } from "node:util";

const { values: cli } = parseArgs({
	options: {
		interactive: { type: "boolean", short: "i" },
		config:      { type: "string",  short: "c" },
		provider:    { type: "string",  short: "p" },
		model:       { type: "string",  short: "m" },
		port:        { type: "string" },
		host:        { type: "string" },
		"chat-id":   { type: "string" },
		help:        { type: "boolean", short: "h" },
	},
	strict: true,
	allowPositionals: false,
});

if (cli.help) {
	console.log(`nexal — multi-channel AI agent orchestrator

Usage:
  nexal [options]        Start the daemon (default)
  nexal -i [options]     Launch interactive TUI client

Daemon options:
  -c, --config <path>     Config file path   (env: NEXAL_CONFIG_PATH)
  -p, --provider <name>   Model provider     (env: NEXAL_MODEL_PROVIDER, default: openrouter)
  -m, --model <id>        Model id           (env: NEXAL_MODEL, default: openai/gpt-4o)
      --port <number>     HTTP listen port   (env: NEXAL_HTTP_PORT, default: 3000)

TUI options (-i):
      --host <addr>       WebSocket host     (default: 127.0.0.1)
      --port <number>     WebSocket port     (default: 3001)
      --chat-id <id>      Chat session ID    (default: tui)

General:
  -h, --help              Show this help

All options can also be set via environment variables or ~/.nexal/config.toml.
Priority: CLI flags > env vars > config file > defaults.`);
	process.exit(0);
}

if (cli.interactive) {
	const wsHost = cli.host ?? "127.0.0.1";
	const wsPort = Number(cli.port ?? "3001");

	// Check if the daemon is already running by probing the WS port.
	const daemonUp = await new Promise<boolean>((resolve) => {
		const ws = new WebSocket(`ws://${wsHost}:${wsPort}/ws`);
		const t = setTimeout(() => { ws.close(); resolve(false); }, 1_000);
		ws.addEventListener("open", () => { clearTimeout(t); ws.close(); resolve(true); });
		ws.addEventListener("error", () => { clearTimeout(t); resolve(false); });
	});

	if (!daemonUp) {
		// Auto-start daemon in background. Pass through config/provider/model
		// flags so the daemon picks up the same settings.
		const args: string[] = [];
		if (cli.config)   args.push("-c", cli.config);
		if (cli.provider) args.push("-p", cli.provider);
		if (cli.model)    args.push("-m", cli.model);

		const { spawn } = await import("bun");
		const daemon = spawn({
			cmd: [process.execPath, ...args],
			stdout: "ignore",
			stderr: "ignore",
			stdin: "ignore",
		});
		daemon.unref();

		// Wait until the WS port is ready (up to 15s).
		const deadline = Date.now() + 15_000;
		let ready = false;
		while (Date.now() < deadline) {
			ready = await new Promise<boolean>((resolve) => {
				const ws = new WebSocket(`ws://${wsHost}:${wsPort}/ws`);
				const t = setTimeout(() => { ws.close(); resolve(false); }, 1_000);
				ws.addEventListener("open", () => { clearTimeout(t); ws.close(); resolve(true); });
				ws.addEventListener("error", () => { clearTimeout(t); resolve(false); });
			});
			if (ready) break;
			await new Promise((r) => setTimeout(r, 500));
		}
		if (!ready) {
			console.error("failed to start nexal daemon within 15s");
			process.exit(1);
		}
	}

	// TUI mode — pass args via env vars so tui.ts skips its own parseArgs.
	process.env.NEXAL_TUI_EMBEDDED = "1";
	if (cli.host) process.env.NEXAL_TUI_HOST = cli.host;
	if (cli.port) process.env.NEXAL_TUI_PORT = cli.port;
	if (cli["chat-id"]) process.env.NEXAL_TUI_CHAT_ID = cli["chat-id"];
	await import("./tui.ts");
} else {
	// Daemon mode — apply CLI flags as env vars, then start.
	if (cli.config)   process.env.NEXAL_CONFIG_PATH = cli.config;
	if (cli.provider) process.env.NEXAL_MODEL_PROVIDER = cli.provider;
	if (cli.model)    process.env.NEXAL_MODEL = cli.model;
	if (cli.port)     process.env.NEXAL_HTTP_PORT = cli.port;

	const { main } = await import("./index.ts");
	const { log } = await import("./log.ts");
	main().catch((err) => {
		log.error("fatal error, exiting", err);
		process.exit(1);
	});
}
