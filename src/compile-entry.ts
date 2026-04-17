#!/usr/bin/env bun
/**
 * Compile-only entry point for `bun build --compile`.
 *
 * This file imports the Rust binaries and PGlite WASM assets with
 * { type: "file" } so they get embedded into the single executable.
 * It registers them with the embedded module and delegates to the CLI.
 */

// Embed Rust binaries at bundle time.
// @ts-expect-error — binary file imports are not typed
import gatewayBin from "../target/release/nexal-gateway" with { type: "file" };
// @ts-expect-error
import agentBin from "../target/release/nexal-agent" with { type: "file" };

// Embed PGlite WASM/data assets at bundle time.
// @ts-expect-error
import pgliteWasm from "../node_modules/@electric-sql/pglite/dist/pglite.wasm" with { type: "file" };
// @ts-expect-error
import pgliteData from "../node_modules/@electric-sql/pglite/dist/pglite.data" with { type: "file" };
// @ts-expect-error
import initdbWasm from "../node_modules/@electric-sql/pglite/dist/initdb.wasm" with { type: "file" };

import { setEmbeddedPaths } from "./embedded.ts";

setEmbeddedPaths({
	gateway: gatewayBin,
	agent: agentBin,
	pgliteWasm,
	pgliteData,
	initdbWasm,
});

// Delegate to the unified CLI.
await import("./cli.ts");
