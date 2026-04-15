/**
 * Sandbox factory — picks a backend by name from config.
 *
 * Sandboxing is mandatory; only the *backend* is configurable. Today the
 * only backend is `"podman"`. Adding e.g. `"firecracker"` is a matter
 * of writing another `SandboxBackend` impl and registering it here.
 */

import { PodmanBackend, type PodmanBackendConfig } from "./podman.ts";
import type { SandboxBackend } from "./types.ts";

export type { SandboxBackend } from "./types.ts";
export { PodmanBackend, type PodmanBackendConfig } from "./podman.ts";

export interface SandboxFactoryOptions {
	/** Backend identifier. Default: "podman" (the only one implemented). */
	backend?: string;
	/** Backend-specific config bucket (passed straight through to the chosen backend). */
	config: Record<string, unknown>;
	/** Resolved fallback values pulled from the top-level NexalConfig. */
	defaults: { execServerBin: string; workspace: string };
}

export function createSandboxBackend(opts: SandboxFactoryOptions): SandboxBackend {
	const name = (opts.backend ?? "podman").toLowerCase();
	switch (name) {
		case "podman":
			return new PodmanBackend(buildPodmanConfig(opts));
		default:
			throw new Error(
				`unknown sandbox backend: "${name}". Supported: "podman".`,
			);
	}
}

function buildPodmanConfig(opts: SandboxFactoryOptions): PodmanBackendConfig {
	const c = opts.config;
	return {
		image:
			(c.image as string | undefined) ??
			"ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13",
		execServerBin: (c.execServerBin as string | undefined) ?? opts.defaults.execServerBin,
		podmanBin: c.podmanBin as string | undefined,
		runtime: c.runtime as string | undefined,
		memory: (c.memory as string | undefined) ?? "512m",
		cpus: (c.cpus as string | undefined) ?? "1.0",
		pidsLimit: (c.pidsLimit as number | undefined) ?? 256,
		network: (c.network as boolean | undefined) ?? true,
		workspace: (c.workspace as string | undefined) ?? opts.defaults.workspace,
	};
}
