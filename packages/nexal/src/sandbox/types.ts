/**
 * SandboxBackend — pluggable per-session sandbox.
 *
 * Every sandbox backend produces a fresh `ExecServerClient` per session
 * key. The client speaks the exec-server stdio JSON-RPC protocol; how
 * those bytes get delivered into a sandboxed environment (podman exec,
 * gvisor, firecracker, plain process, …) is up to the implementation.
 *
 * Lifecycle:
 *   - `acquire(key)` — return an unconnected `ExecServerClient`.
 *     Idempotent per `key`: subsequent calls for the same `key`
 *     before `release(key)` return clients pointing at the same
 *     underlying sandbox.
 *   - `release(key)` — tear down the sandbox for `key`. Safe if no
 *     sandbox exists for that key.
 *   - `releaseAll()` — release everything (called on shutdown).
 *
 * Backends MUST be safe to call concurrently from many sessions.
 */

import type { ExecServerClient } from "../exec-client.ts";

export interface SandboxBackend {
	/** Backend identifier, e.g. `"podman"`. */
	readonly name: string;

	acquire(sessionKey: string): Promise<ExecServerClient>;
	release(sessionKey: string): Promise<void>;
	releaseAll(): Promise<void>;
	/**
	 * Optional: forget the mapping for `sessionKey` but leave the
	 * sandbox running. Used by long-lived sub-agent tasks so their
	 * containers survive a nexal process restart. Backends that don't
	 * implement this MAY fall back to no-op (mapping stays; next
	 * `acquire` will skip any "reuse" branch the backend implements).
	 */
	detach?(sessionKey: string): Promise<void>;
}
