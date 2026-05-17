#!/usr/bin/env bun
/**
 * Compile-only entry point for `bun build --compile`.
 *
 * Imports the Rust binaries and drizzle migration files with
 * { type: "file" } so they get embedded into the single executable,
 * registers them with the embedded module, and delegates to the CLI.
 *
 * NOTE: when you add a new migration via `bun run db:generate`, add its
 * `.sql` file to the imports + `migrations` array below so the compiled
 * binary can apply it. (`meta/_journal.json` lists every migration; the
 * runtime migrator reads it + the .sql files.)
 */

// Embed Rust binaries at bundle time.
// @ts-expect-error — binary file imports are not typed
import gatewayBin from "../target/release/nexal-gateway" with { type: "file" };
// @ts-expect-error
import agentBin from "../target/release/nexal-agent" with { type: "file" };

// Embed drizzle migrations at bundle time.
import mJournal from "../drizzle/meta/_journal.json" with { type: "file" };
// @ts-expect-error — .sql file imports are not typed
import m0000 from "../drizzle/0000_colossal_silver_surfer.sql" with { type: "file" };

import { setEmbeddedPaths } from "./embedded.ts";

setEmbeddedPaths({
	gateway: gatewayBin,
	agent: agentBin,
	migrations: [
		{ name: "meta/_journal.json", path: mJournal },
		{ name: "0000_colossal_silver_surfer.sql", path: m0000 },
	],
});

// Delegate to the unified CLI.
await import("./cli.ts");
