/**
 * Embedded resource handling for `bun build --compile` builds.
 *
 * In compiled mode, prompts are inlined as strings and Rust binaries are
 * embedded in Bun's virtual /$bunfs/ filesystem. Since binaries in $bunfs
 * cannot be exec'd directly, we extract them to ~/.nexal/bin/ on first run.
 *
 * In dev mode (`bun run`), everything is read from disk as before.
 */
import { writeFileSync, chmodSync, mkdirSync, existsSync, statSync, renameSync } from "node:fs";
import { join, dirname } from "node:path";
import { homedir } from "node:os";
import { log } from "./log.ts";

// ── Compile-time detection ─────────────────────────────────────────

export const isCompiled = import.meta.path.startsWith("/$bunfs/");

// ── Embedded prompts (always inlined at bundle time) ───────────────

import COORDINATOR_PROMPT from "./prompts/coordinator.md" with { type: "text" };
import EXECUTOR_PROMPT from "./prompts/executor.md" with { type: "text" };
export { COORDINATOR_PROMPT, EXECUTOR_PROMPT };

// ── Embedded assets (set by compile-only entry) ────────────────────
// The compiled entry (src/compile-entry.ts) assigns these before main()
// runs. In dev mode they stay null and everything reads from disk.

export let embeddedGatewayPath: string | null = null;
export let embeddedAgentPath: string | null = null;

/** Drizzle migration files, relative name → embedded $bunfs path. */
export interface EmbeddedMigration {
	/** Path relative to the `drizzle/` folder, e.g. "meta/_journal.json". */
	name: string;
	/** $bunfs path of the embedded file. */
	path: string;
}
export let embeddedMigrations: EmbeddedMigration[] | null = null;

export function setEmbeddedPaths(paths: {
	gateway?: string | null;
	agent?: string | null;
	migrations?: EmbeddedMigration[] | null;
}): void {
	embeddedGatewayPath = paths.gateway ?? null;
	embeddedAgentPath = paths.agent ?? null;
	embeddedMigrations = paths.migrations ?? null;
}

// ── Extraction ─────────────────────────────────────────────────────

const CACHE_DIR = join(homedir(), ".nexal", "bin");

async function extract(
	embeddedPath: string,
	name: string,
	selfMtime: number,
	dir: string = CACHE_DIR,
	quiet: boolean = false,
): Promise<string> {
	const dest = join(dir, name);

	if (existsSync(dest)) {
		const destMtime = statSync(dest).mtimeMs;
		if (destMtime >= selfMtime) {
			if (!quiet) log.info(`${name} cached at ${dest}`);
			return dest;
		}
	}

	if (!quiet) log.info(`extracting embedded ${name} → ${dest}`);
	const buf = await Bun.file(embeddedPath).arrayBuffer();
	try {
		writeFileSync(dest, Buffer.from(buf));
	} catch (err: any) {
		if (err?.code === "ETXTBSY") {
			// Binary is currently running — write to a temp file and rename
			// (atomic swap works even when the old inode is busy).
			const tmp = dest + `.tmp.${process.pid}`;
			writeFileSync(tmp, Buffer.from(buf));
			chmodSync(tmp, 0o755);
			renameSync(tmp, dest);
			return dest;
		}
		throw err;
	}
	chmodSync(dest, 0o755);
	return dest;
}

/**
 * Extract embedded Rust binaries to disk and return their paths.
 * Returns nulls in dev mode (not compiled).
 */
export async function extractEmbeddedBinaries(): Promise<{
	gatewayBin: string | null;
	agentBin: string | null;
}> {
	if (!isCompiled || (!embeddedGatewayPath && !embeddedAgentPath)) {
		return { gatewayBin: null, agentBin: null };
	}

	mkdirSync(CACHE_DIR, { recursive: true });
	const selfMtime = statSync(process.execPath).mtimeMs;

	const [gatewayBin, agentBin] = await Promise.all([
		embeddedGatewayPath ? extract(embeddedGatewayPath, "nexal-gateway", selfMtime) : null,
		embeddedAgentPath ? extract(embeddedAgentPath, "nexal-agent", selfMtime) : null,
	]);

	return { gatewayBin, agentBin };
}

// ── Migration extraction (compiled mode) ───────────────────────────

const MIG_DIR = join(homedir(), ".nexal", "migrations");

/**
 * Extract embedded drizzle migration files to ~/.nexal/migrations/
 * (preserving the `meta/` subdir layout the migrator expects) and
 * return that folder. Null in dev mode — callers use ./drizzle then.
 */
export async function extractMigrations(): Promise<string | null> {
	if (!isCompiled || !embeddedMigrations) return null;

	for (const m of embeddedMigrations) {
		const dest = join(MIG_DIR, m.name);
		mkdirSync(dirname(dest), { recursive: true });
		const buf = await Bun.file(m.path).arrayBuffer();
		writeFileSync(dest, Buffer.from(buf));
	}
	return MIG_DIR;
}
