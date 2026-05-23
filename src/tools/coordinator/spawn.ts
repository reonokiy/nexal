import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static } from "@mariozechner/pi-ai";

import type { WorkerRegistry } from "../../workers/registry.ts";
import type { SpawnRequest } from "../../workers/registry.ts";
import type { SendPolicy } from "../../workers/store.ts";

import type { CoordinatorCtx } from "./schemas.ts";
import { SpawnCoordinatorParams, SpawnExecutorParams, SpawnOneshotParams } from "./schemas.ts";

// ── Shared helper ─────────────────────────────────────────────────────

async function spawnAndReport(
	registry: WorkerRegistry,
	req: SpawnRequest,
): Promise<AgentToolResult<{ id: string; status: string }>> {
	const row = await registry.spawn(req);
	return {
		content: [
			{
				type: "text",
				text: `spawned ${req.kind} (${req.lifetime}) id=${row.id} name=${row.name} status=${row.status}`,
			},
		],
		details: { id: row.id, status: row.status },
	};
}

// ── Tools ─────────────────────────────────────────────────────────────

export function spawnExecutorTool(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<typeof SpawnExecutorParams, { id: string; status: string }> {
	return {
		name: "spawn_executor",
		label: "Spawn Executor",
		description:
			"Create a long-lived executor agent with its own Podman sandbox, bash, and " +
			"send_update. The executor persists across many turns — use route_to_agent to " +
			"feed it new instructions. Use this when an ongoing project area benefits from " +
			"accumulated context.",
		parameters: SpawnExecutorParams,
		async execute(
			_id: string,
			params: Static<typeof SpawnExecutorParams>,
		): Promise<AgentToolResult<{ id: string; status: string }>> {
			return spawnAndReport(registry, {
				kind: "executor",
				lifetime: "persistent",
				parentSessionKey: ctx.parentSessionKey,
				sourceChannel: ctx.sourceChannel,
				sourceChatId: ctx.sourceChatId,
				sourceReplyTo: ctx.sourceReplyTo ?? null,
				name: params.name,
				initialPrompt: params.initial_prompt,
				systemPrompt: params.system_prompt,
				sendPolicy: (params.send_policy as SendPolicy | undefined) ?? "explicit",
			});
		},
	};
}

export function spawnOneshotTool(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<typeof SpawnOneshotParams, { id: string; status: string }> {
	return {
		name: "spawn_oneshot",
		label: "Spawn Oneshot",
		description:
			"Create a one-shot executor that runs the given prompt once and terminates on " +
			"completion. Use for self-contained jobs (a build, a single refactor, a data " +
			"fetch) where no follow-up routing is expected.",
		parameters: SpawnOneshotParams,
		async execute(
			_id: string,
			params: Static<typeof SpawnOneshotParams>,
		): Promise<AgentToolResult<{ id: string; status: string }>> {
			return spawnAndReport(registry, {
				kind: "executor",
				lifetime: "oneshot",
				parentSessionKey: ctx.parentSessionKey,
				sourceChannel: ctx.sourceChannel,
				sourceChatId: ctx.sourceChatId,
				sourceReplyTo: ctx.sourceReplyTo ?? null,
				name: params.name,
				initialPrompt: params.prompt,
				systemPrompt: params.system_prompt,
				sendPolicy: (params.send_policy as SendPolicy | undefined) ?? "explicit",
			});
		},
	};
}

export function spawnCoordinatorTool(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<typeof SpawnCoordinatorParams, { id: string; status: string }> {
	return {
		name: "spawn_coordinator",
		label: "Spawn Coordinator",
		description:
			"Create a long-lived sub-coordinator. Sub-coordinators are themselves " +
			"dispatchers: they own a subtree of executors (and possibly more " +
			"sub-coordinators), and they respond to route_to_agent calls. Use this when " +
			"a domain is large enough to deserve its own scheduling layer (e.g. a project " +
			"with many specialist executors).",
		parameters: SpawnCoordinatorParams,
		async execute(
			_id: string,
			params: Static<typeof SpawnCoordinatorParams>,
		): Promise<AgentToolResult<{ id: string; status: string }>> {
			return spawnAndReport(registry, {
				kind: "coordinator",
				lifetime: "persistent",
				parentSessionKey: ctx.parentSessionKey,
				sourceChannel: ctx.sourceChannel,
				sourceChatId: ctx.sourceChatId,
				sourceReplyTo: ctx.sourceReplyTo ?? null,
				name: params.name,
				initialPrompt: params.initial_prompt,
				systemPrompt: params.system_prompt,
				sendPolicy: "explicit",
			});
		},
	};
}
