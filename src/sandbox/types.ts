/**
 * SandboxBackend + AgentClient abstractions.
 *
 * `AgentClient` is whatever the bash tool needs to run a command in a
 * sandboxed environment. It's intentionally tiny — `runCommand` and
 * `close` — so different transports can satisfy it (today: a wrapper
 * over `nexal-gateway`; potentially: a direct WebSocket to a local
 * `nexal-agent`).
 *
 * `SandboxBackend` owns the lifecycle of the underlying sandbox(es)
 * and hands out `AgentClient` instances on demand. One `acquire(key)`
 * call → one logically isolated workspace.
 *
 * Lifecycle:
 *   - `acquire(key, opts?)` — get a ready-to-use `AgentClient`.
 *     Idempotent per `key`: subsequent calls for the same `key`
 *     before `release(key)` return clients pointing at the same
 *     underlying sandbox.
 *   - `release(key)` — tear down the sandbox for `key`. Safe if no
 *     sandbox exists for that key.
 *   - `releaseAll()` — release everything (called on shutdown).
 *   - `detach(key)` — drop the in-memory mapping but keep the sandbox
 *     alive (for tasks that must survive a nexal restart). Optional;
 *     not all backends support it.
 *
 * Backends MUST be safe to call concurrently from many sessions.
 */

export interface RunCommandOptions {
	cwd?: string;
	env?: Record<string, string>;
	timeoutMs?: number;
	processId?: string;
}

export interface RunCommandResult {
	stdout: string;
	stderr: string;
	exitCode: number;
	timedOut: boolean;
}

/**
 * What the bash tool consumes. Concrete impls today: `GatewayAgentClient`.
 */
export interface AgentClient {
	/**
	 * Backend-specific stable id for this agent. Gateway-backed clients
	 * set it; local/file-based backends may leave it undefined.
	 */
	readonly agentId?: string;
	/** Run a command and accumulate output until exit. */
	runCommand(argv: string[], opts?: RunCommandOptions): Promise<RunCommandResult>;
	/** Close any per-client resources. Does NOT kill the underlying sandbox. */
	close(): Promise<void>;
}

export interface AcquireOptions {
	/** Extra env vars passed into the sandbox. */
	env?: Record<string, string>;
}

export interface SandboxBackend {
	/** Backend identifier, e.g. `"gateway"`. */
	readonly name: string;

	acquire(sessionKey: string, opts?: AcquireOptions): Promise<AgentClient>;
	release(sessionKey: string): Promise<void>;
	releaseAll(): Promise<void>;
	/**
	 * Optional: forget the mapping for `sessionKey` but leave the
	 * sandbox alive. Used by long-lived sub-agent tasks so their
	 * containers survive a nexal process restart.
	 */
	detach?(sessionKey: string): Promise<void>;
}
