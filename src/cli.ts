#!/usr/bin/env bun
/**
 * nexal unified CLI entry point.
 *
 *   nexal           → start the daemon (gateway + channels + agent pool)
 *   nexal --help    → show help
 */
import { parseArgs } from "node:util";

const { values: cli } = parseArgs({
	options: {
		config:   { type: "string",  short: "c" },
		provider: { type: "string",  short: "p" },
		model:    { type: "string",  short: "m" },
		port:     { type: "string" },
		help:     { type: "boolean", short: "h" },
	},
	strict: true,
	allowPositionals: false,
});

if (cli.help) {
	console.log(`nexal — multi-channel AI agent orchestrator

Usage:
  nexal [options]        Start the daemon

Daemon options:
  -c, --config <path>     Config file path   (env: NEXAL_CONFIG_PATH)
  -p, --provider <name>   Model provider     (env: NEXAL_MODEL_PROVIDER, default: openrouter)
  -m, --model <id>        Model id           (env: NEXAL_MODEL, default: openai/gpt-4o)
      --port <number>     HTTP listen port   (env: NEXAL_HTTP_PORT, default: 3000)

General:
  -h, --help              Show this help

All options can also be set via environment variables or ~/.nexal/config.toml.
Priority: CLI flags > env vars > config file > defaults.`);
	process.exit(0);
}

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
