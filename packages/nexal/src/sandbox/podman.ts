/**
 * Podman sandbox backend.
 *
 * TS port of `crates/nexal/src/podman.rs` (`create_sandbox_container`).
 *
 * ## Architecture
 *
 * Each `sessionKey` (e.g. `"telegram:-100123456"`) gets a long-lived
 * Podman container named `nexal-<sanitized-session-key>`. At create
 * time:
 *
 *   1. The host-side `nexal-exec-server` binary is copied into the
 *      container at `/usr/local/bin/`.
 *   2. The container is started with that binary as PID 1, listening
 *      on `ws://0.0.0.0:9100` inside the container.
 *   3. Port 9100 is published to a random host port via Podman's port
 *      mapping; we discover it with `podman inspect`.
 *   4. The TS side connects to `ws://127.0.0.1:<host-port>` with the
 *      WebSocket-based ExecServerClient.
 *
 * The container stays up for the life of the session; the WS
 * connection persists too. A new `acquire(sameKey)` returns a fresh
 * client to the same container.
 *
 * ## Why podman CLI and not the REST API
 *
 * The Rust implementation uses `bollard` (Docker/Podman REST). From
 * Bun we'd need a Docker API client; the CLI is already installed and
 * well-behaved. Overhead is a few spawns per container lifecycle,
 * which is a drop in the bucket. Migrate to REST when it matters.
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
/** Port exec-server listens on inside the container. */
const CONTAINER_WS_PORT = 9100;

interface ContainerInfo {
	name: string;
	wsUrl: string;
}

export class PodmanBackend implements SandboxBackend {
	readonly name = "podman";
	private readonly containers = new Map<string, ContainerInfo>();
	private readonly acquiring = new Map<string, Promise<ContainerInfo>>();

	constructor(private readonly config: PodmanBackendConfig) {}

	async acquire(sessionKey: string): Promise<ExecServerClient> {
		const info = await this.ensureContainer(sessionKey);
		return new ExecServerClient({ url: info.wsUrl });
	}

	async release(sessionKey: string): Promise<void> {
		const info = this.containers.get(sessionKey);
		if (!info) return;
		await this.podman(["rm", "-f", info.name]).catch(() => undefined);
		this.containers.delete(sessionKey);
	}

	async releaseAll(): Promise<void> {
		await Promise.all([...this.containers.keys()].map((k) => this.release(k)));
	}

	// ── Internals ─────────────────────────────────────────────────────

	private async ensureContainer(sessionKey: string): Promise<ContainerInfo> {
		const existing = this.containers.get(sessionKey);
		if (existing) return existing;

		const inflight = this.acquiring.get(sessionKey);
		if (inflight) return inflight;

		const p = this.createContainer(sessionKey).finally(() => this.acquiring.delete(sessionKey));
		this.acquiring.set(sessionKey, p);
		const info = await p;
		this.containers.set(sessionKey, info);
		return info;
	}

	private async createContainer(sessionKey: string): Promise<ContainerInfo> {
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
			// Publish the in-container exec-server WS port to a random
			// host port, bound to localhost. Format: HOST_IP:HOST_PORT:CTR_PORT.
			// HOST_PORT=0 → kernel assigns.
			`--publish=127.0.0.1::${CONTAINER_WS_PORT}/tcp`,
		];

		if (this.config.runtime) args.push(`--runtime=${this.config.runtime}`);
		if (this.config.memory) args.push(`--memory=${this.config.memory}`);
		if (this.config.cpus) args.push(`--cpus=${this.config.cpus}`);
		if (this.config.pidsLimit !== undefined) args.push(`--pids-limit=${this.config.pidsLimit}`);
		// Always use pasta — we need a network namespace for the published WS
		// port to be reachable from the host. The `network` flag only toggles
		// outbound DNS resolution.
		args.push("--network=pasta");
		if (this.config.network) {
			args.push("--dns=1.1.1.1", "--dns=8.8.8.8");
		}
		if (this.config.workspace) args.push(`--volume=${this.config.workspace}:/workspace`);
		args.push(this.config.image);
		// Container entrypoint = exec-server itself, listening on WS.
		args.push("/usr/local/bin/nexal-exec-server", "--listen", `ws://0.0.0.0:${CONTAINER_WS_PORT}`);

		await this.podman(args);

		// Copy exec-server binary into the container at /usr/local/bin.
		await this.podman([
			"cp",
			this.config.execServerBin,
			`${name}:/usr/local/bin/nexal-exec-server`,
		]);

		await this.podman(["start", name]);

		// Discover the host-mapped port via `podman port`.
		const wsUrl = await this.discoverWsUrl(name);
		return { name, wsUrl };
	}

	/** Wait for podman to publish the port, then resolve `ws://127.0.0.1:<host-port>`. */
	private async discoverWsUrl(containerName: string): Promise<string> {
		for (let attempt = 1; attempt <= 30; attempt++) {
			try {
				// `podman port <name> 9100/tcp` prints e.g. "127.0.0.1:34567"
				const out = await this.podman(["port", containerName, `${CONTAINER_WS_PORT}/tcp`]);
				const line = out.split("\n")[0]?.trim();
				if (line) {
					return `ws://${line}`;
				}
			} catch {
				// fall through to retry
			}
			await new Promise((r) => setTimeout(r, 200));
		}
		throw new Error(`could not discover host-mapped WS port for container ${containerName}`);
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
