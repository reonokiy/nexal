/**
 * Sandbox factory — picks a backend by name from config.
 *
 * Today the only backend is `"gateway"` (talks to `nexal-gateway`,
 * which in turn owns podman). Adding e.g. a `"local"` backend that
 * spawns `nexal-agent` directly without a container is a matter of
 * writing another `SandboxBackend` impl and registering it here.
 */

import { GatewayBackend, type GatewayBackendOptions } from "./gateway.ts";
import type { GatewayClient } from "../gateway/client.ts";
import type { SandboxBackend } from "./types.ts";

export type {
	AcquireOptions,
	AgentClient,
	RunCommandOptions,
	RunCommandResult,
	SandboxBackend,
} from "./types.ts";
export { GatewayBackend, type GatewayBackendOptions } from "./gateway.ts";

export interface SandboxFactoryOptions {
	/** Backend identifier. Default: `"gateway"`. */
	backend?: string;
	/** Shared `GatewayClient` used by the gateway backend. */
	gatewayClient: GatewayClient;
	/** Backend-specific options bag (currently only `gateway` consumes it). */
	gatewayOptions?: GatewayBackendOptions;
}

export function createSandboxBackend(opts: SandboxFactoryOptions): SandboxBackend {
	const name = (opts.backend ?? "gateway").toLowerCase();
	switch (name) {
		case "gateway":
			return new GatewayBackend(opts.gatewayClient, opts.gatewayOptions);
		default:
			throw new Error(`unknown sandbox backend: "${name}". Supported: "gateway".`);
	}
}
