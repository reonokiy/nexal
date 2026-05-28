import type { AgentTool } from "@mariozechner/pi-agent-core";

import type { WorkerRegistry } from "../../workers/registry.ts";

import type { CoordinatorCtx } from "./schemas.ts";
import { cancelAgentTool, getAgentTool, listAgentsTool, sendToAgentTool } from "./manage.ts";
import { spawnCoordinatorTool, spawnExecutorTool, spawnOneshotTool } from "./spawn.ts";

export type { CoordinatorCtx } from "./schemas.ts";

export function createCoordinatorTools(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<any>[] {
	return [
		spawnExecutorTool(registry, ctx),
		spawnOneshotTool(registry, ctx),
		spawnCoordinatorTool(registry, ctx),
		sendToAgentTool(registry, ctx),
		listAgentsTool(registry, ctx),
		getAgentTool(registry),
		cancelAgentTool(registry),
	] as AgentTool<any>[];
}
