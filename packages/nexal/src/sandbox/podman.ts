/**
 * Podman sandbox backend.
 *
 * TS port of `crates/nexal/src/podman.rs` (`create_sandbox_container`).
 *
 * ## Architecture
 *
 * Each `sessionKey` (e.g. `"telegram:-100123456"`) gets a long-lived
 * Podman container named `nexal-<sanitized-session-key>`. Inside the
 * container, `/usr/local/bin/nexal-exec-server` is present (injected
 * at container creation). When the agent wants to run a shell command:
 *
 *     podman exec -i <container> /usr/local/bin/nexal-exec-server --listen stdio
 *
 * is spawned as a child process. stdio carries the JSON-RPC messages
 * (same protocol as `ExecServerClient`). The container stays up
 * between tool calls so the bash environment persists across turns.
 *
 * ## Why podman CLI and not the REST API
 *
 * The Rust implementation uses `bollard` (Docker/Podman REST). From
 * Bun we'd need a Docker API client; the CLI is already installed and
 * well-behaved. Overhead is a few spawns per container lifecycle,
 * which is a drop in the bucket. Migrate to REST when it matters.
 *
 * ## Why stdio inside the container, not the exec-server WebSocket
 *
 * The openai-oss-forks tungstenite fork in exec-server refuses stock
 * WebSocket clients (see `project_exec_server_stdio.md` memory). stdio
 * works unchanged; `podman exec -i` wires it through.
 */

import { ExecServerClient } from "../exec-client.ts";
import type { SandboxBackend } from "./types.ts";

export interface PodmanBackendConfig {
	/** Podman image (e.g. "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13"). */
	image: string;
	/** Path to the host-side nexal-exec-server binary (copied into each container). */
	execServerBin: string;
	/** Path to the podman CLI. Default: "podman". */
	podmanBin?: string;
	/** OCI runtime override (e.g. "crun"). */
	runtime?: string;
	/** Memory limit (e.g. "512m", "1g"). */
	memory?: string;
	/** CPU limit as a float (e.g. "1.0"). */
	cpus?: string;
	/** PID limit. */
	pidsLimit?: number;
	/** Enable network inside the sandbox. */
	network?: boolean;
	/** Host directory bind-mounted at /workspace inside the container. */
	workspace?: string;
}

const DEFAULT_PODMAN = "podman";

export class PodmanBackend implements SandboxBackend {
	readonly name = "podman";
	private readonly containers = new Map<string, string>(); // sessionKey → container name
	private readonly acquiring = new Map<string, Promise<string>>();

	constructor(private readonly config: PodmanBackendConfig) {}

	async acquire(sessionKey: string): Promise<ExecServerClient> {
		const name = await this.ensureContainer(sessionKey);
		return new ExecServerClient({
			cmd: [
				this.config.podmanBin ?? DEFAULT_PODMAN,
				"exec",
				"-i",
				name,
				"/usr/local/bin/nexal-exec-server",
				"--listen",
				"stdio",
			],
		});
	}

	async release(sessionKey: string): Promise<void> {
		const name = this.containers.get(sessionKey);
		if (!name) return;
		await this.podman(["rm", "-f", name]).catch(() => undefined);
		this.containers.delete(sessionKey);
	}

	async releaseAll(): Promise<void> {
		await Promise.all([...this.containers.keys()].map((k) => this.release(k)));
	}

	// ── Internals ─────────────────────────────────────────────────────

	private async ensureContainer(sessionKey: string): Promise<string> {
		const existing = this.containers.get(sessionKey);
		if (existing) return existing;

		const inflight = this.acquiring.get(sessionKey);
		if (inflight) return inflight;

		const p = this.createContainer(sessionKey).finally(() => this.acquiring.delete(sessionKey));
		this.acquiring.set(sessionKey, p);
		const name = await p;
		this.containers.set(sessionKey, name);
		return name;
	}

	private async createContainer(sessionKey: string): Promise<string> {
		const name = `nexal-${sanitize(sessionKey)}`;

		// Wipe any stale container from a previous run.
		await this.podman(["rm", "-f", name]).catch(() => undefined);

		const args: string[] = [
			"create",
			"--name",
			name,
			"--userns=keep-id",
			"--security-opt=no-new-privileges",
			"--cap-drop=ALL",
			"--env=HOME=/workspace",
			"--workdir=/workspace",
		];

		if (this.config.runtime) args.push(`--runtime=${this.config.runtime}`);
		if (this.config.memory) args.push(`--memory=${this.config.memory}`);
		if (this.config.cpus) args.push(`--cpus=${this.config.cpus}`);
		if (this.config.pidsLimit !== undefined) args.push(`--pids-limit=${this.config.pidsLimit}`);
		args.push(this.config.network ? "--network=pasta" : "--network=none");
		if (this.config.network) {
			args.push("--dns=1.1.1.1", "--dns=8.8.8.8");
		}
		if (this.config.workspace) args.push(`--volume=${this.config.workspace}:/workspace`);
		args.push(this.config.image);
		// Entrypoint keeps the container alive; we `podman exec` into it
		// for each JSON-RPC session.
		args.push("sleep", "infinity");

		await this.podman(args);

		// Copy exec-server binary into the container at /usr/local/bin.
		await this.podman([
			"cp",
			this.config.execServerBin,
			`${name}:/usr/local/bin/nexal-exec-server`,
		]);

		await this.podman(["start", name]);
		return name;
	}

	private async podman(args: string[]): Promise<string> {
		const bin = this.config.podmanBin ?? DEFAULT_PODMAN;
		const proc = Bun.spawn({ cmd: [bin, ...args], stdout: "pipe", stderr: "pipe" });
		const [stdout, stderr, code] = await Promise.all([
			new Response(proc.stdout).text(),
			new Response(proc.stderr).text(),
			proc.exited,
		]);
		if (code !== 0) throw new Error(`podman ${args.join(" ")} → exit ${code}: ${stderr.trim()}`);
		return stdout;
	}
}

/** Sanitize a session key for use as a container name. */
function sanitize(key: string): string {
	return key.replace(/[^a-zA-Z0-9_.-]/g, "_");
}
