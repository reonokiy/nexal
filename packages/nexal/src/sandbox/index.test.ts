import { describe, expect, test } from "bun:test";

import type { GatewayClient } from "../gateway/client.ts";
import { createSandboxBackend } from "./index.ts";
import { GatewayBackend } from "./gateway.ts";

function dummyGateway(): GatewayClient {
	return {
		hello: async () => undefined,
		invoke: async () => ({}) as any,
		invokeAgent: async () => ({}) as any,
		connect: async () => undefined,
		subscribe: () => () => undefined,
		close: async () => undefined,
	} as unknown as GatewayClient;
}

describe("createSandboxBackend", () => {
	test("defaults to gateway backend", () => {
		const backend = createSandboxBackend({ gatewayClient: dummyGateway() });
		expect(backend).toBeInstanceOf(GatewayBackend);
		expect(backend.name).toBe("gateway");
	});

	test("explicit backend=\"gateway\" is also accepted", () => {
		const backend = createSandboxBackend({
			backend: "gateway",
			gatewayClient: dummyGateway(),
		});
		expect(backend).toBeInstanceOf(GatewayBackend);
	});

	test("backend name is case-insensitive", () => {
		const backend = createSandboxBackend({
			backend: "GATEWAY",
			gatewayClient: dummyGateway(),
		});
		expect(backend).toBeInstanceOf(GatewayBackend);
	});

	test("unknown backend throws a descriptive error", () => {
		expect(() =>
			createSandboxBackend({ backend: "firecracker", gatewayClient: dummyGateway() }),
		).toThrow(/unknown sandbox backend: "firecracker"/);
	});

	test("gatewayOptions are threaded into the backend (defaultWorkspace)", async () => {
		let observed: unknown;
		const gw: GatewayClient = {
			...dummyGateway(),
			invoke: async (_method: string, params: unknown) => {
				observed = params;
				return { agent_id: "x", container_name: "nexal-worker-x" } as any;
			},
		} as any;
		const backend = createSandboxBackend({
			gatewayClient: gw,
			gatewayOptions: { defaultWorkspace: "/opt/ws" },
		});
		await backend.acquire("worker:w");
		// acquire calls spawn_agent; that's the second call after hello
		// (hello is a no-op in our stub), so we just verify workspace
		// made it into the payload.
		expect((observed as any).workspace).toBe("/opt/ws");
	});
});
