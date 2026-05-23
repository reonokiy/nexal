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
		config: { type: "string", short: "c" },
		help:   { type: "boolean", short: "h" },
	},
	strict: true,
	allowPositionals: false,
});

if (cli.help) {
	console.log(`nexal — multi-channel AI agent orchestrator

Usage:
  nexal [options]        Start the daemon

Options:
  -c, --config <path>     Config file path   (env: NEXAL_CONFIG_PATH)
  -h, --help              Show this help

Model/provider/auth are configured exclusively through the DB-backed
settings KV (via the web UI). Channels, storage and gateway secrets
live in ~/.nexal/config.toml or the matching NEXAL_* env vars.`);
	process.exit(0);
}

if (cli.config) process.env.NEXAL_CONFIG_PATH = cli.config;

const { main } = await import("./index.ts");
const { log } = await import("./log.ts");
main().catch((err) => {
	log.error("fatal error, exiting", err);
	process.exit(1);
});
