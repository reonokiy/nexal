/**
 * Coordinator tools — exposed to coordinators (top-level per-chat
 * dispatcher and recursively to sub-coordinators).
 *
 * A coordinator does NO work itself; it only schedules. The full
 * dispatching surface:
 *
 *   spawn_executor    — long-lived executor with bash + send_update
 *   spawn_oneshot     — one-shot executor (dies on agent_end)
 *   spawn_coordinator — long-lived sub-coordinator (recursive dispatcher)
 *   route_to_agent    — feed a new instruction to a persistent agent
 *                       (coordinator or executor)
 *   list_agents       — what's spawned under THIS coordinator
 *   get_agent         — status + transcript tail
 *   cancel_agent      — kill an agent (frees its container if any)
 *
 * `parentSessionKey`, `sourceChannel`, `sourceChatId` are pinned at
 * tool-creation time. For the top-level coordinator these come from
 * the chat session; for a sub-coordinator they come from its own
 * `WorkerAgent` (its id becomes parentSessionKey for its children, so
 * each coordinator sees only its own subtree in `list_agents`).
 */
import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static, Type } from "@mariozechner/pi-ai";

import type { WorkerRegistry } from "../workers/registry.ts";
import type { SendPolicy, WorkerRow } from "../workers/store.ts";
import { UserContentSchema, type UserContent } from "../content.ts";

export interface CoordinatorCtx {
	/**
	 * Identifier used as the `parent_session_key` of agents spawned
	 * here. For the top-level coordinator it's `"<channel>:<chatId>"`;
	 * for a sub-coordinator it's the sub-coordinator's own row id.
	 */
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
}

const SpawnExecutorParams = Type.Object({
	name: Type.String({
		description:
			"Short kebab-case label (e.g. \"refactor-authz\"). Shown as a prefix on " +
			"every chat message the executor emits via send_update.",
	}),
	system_prompt: Type.String({
		description:
			"The executor's persona / capability description. Frozen at spawn time. " +
			"Be specific about role, allowed actions, and reporting style. Tell it to " +
			"call send_update for milestones.",
	}),
	initial_prompt: Type.Optional(
		Type.String({
			description:
				"Optional first user message. Omit to spawn an empty executor that waits " +
				"for a route_to_agent call.",
		}),
	),
	send_policy: Type.Optional(
		Type.Union([Type.Literal("explicit"), Type.Literal("final"), Type.Literal("all")], {
			description:
				"explicit (default) = only send_update reaches chat; final = + last assistant text per turn; all = every assistant turn.",
		}),
	),
});

const SpawnOneshotParams = Type.Object({
	name: Type.String({ description: "Short kebab-case label (chat-message prefix)." }),
	prompt: Type.String({
		description: "Full instructions. The executor runs once and dies on completion.",
	}),
	system_prompt: Type.Optional(
		Type.String({ description: "Optional override of the default executor system prompt." }),
	),
	send_policy: Type.Optional(
		Type.Union([Type.Literal("explicit"), Type.Literal("final"), Type.Literal("all")], {
			description: "Default explicit; pick final to auto-send the executor's last reply.",
		}),
	),
});

const SpawnCoordinatorParams = Type.Object({
	name: Type.String({
		description:
			"Short kebab-case label for the sub-coordinator. Shown if it ever sends " +
			"directly to the user (rare — coordinators normally route, not talk).",
	}),
	system_prompt: Type.String({
		description:
			"The sub-coordinator's identity: what domain it owns, when to spawn vs route, " +
			"what kinds of executors live under it. Frozen at spawn time.",
	}),
	initial_prompt: Type.Optional(
		Type.String({
			description:
				"Optional first user message. Often omitted — sub-coordinators usually start " +
				"idle and wait for the parent to route work to them.",
		}),
	),
});

const RouteParams = Type.Object({
	id: Type.String({ description: "Agent id from spawn_* / list_agents." }),
	content: UserContentSchema,
});

const IdParams = Type.Object({
	id: Type.String({ description: "Agent id." }),
});

const ListParams = Type.Object({});

export function createCoordinatorTools(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<any>[] {
	return [
		spawnExecutorTool(registry, ctx),
		spawnOneshotTool(registry, ctx),
		spawnCoordinatorTool(registry, ctx),
		routeToAgentTool(registry, ctx),
		listAgentsTool(registry, ctx),
		getAgentTool(registry),
		cancelAgentTool(registry),
	];
}

function spawnExecutorTool(
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
			const row = await registry.spawn({
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
			return {
				content: [
					{
						type: "text",
						text: `spawned executor (persistent) id=${row.id} name=${row.name} status=${row.status}`,
					},
				],
				details: { id: row.id, status: row.status },
			};
		},
	};
}

function spawnOneshotTool(
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
			const row = await registry.spawn({
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
			return {
				content: [
					{
						type: "text",
						text: `spawned executor (oneshot) id=${row.id} name=${row.name} status=${row.status}`,
					},
				],
				details: { id: row.id, status: row.status },
			};
		},
	};
}

function spawnCoordinatorTool(
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
			const row = await registry.spawn({
				kind: "coordinator",
				lifetime: "persistent",
				parentSessionKey: ctx.parentSessionKey,
				sourceChannel: ctx.sourceChannel,
				sourceChatId: ctx.sourceChatId,
				sourceReplyTo: ctx.sourceReplyTo ?? null,
				name: params.name,
				initialPrompt: params.initial_prompt,
				systemPrompt: params.system_prompt,
				// Coordinators don't auto-send assistant text to the chat.
				sendPolicy: "explicit",
			});
			return {
				content: [
					{
						type: "text",
						text: `spawned coordinator id=${row.id} name=${row.name} status=${row.status}`,
					},
				],
				details: { id: row.id, status: row.status },
			};
		},
	};
}

function routeToAgentTool(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<typeof RouteParams, { id: string }> {
	return {
		name: "route_to_agent",
		label: "Route To Agent",
		description:
			"Feed a new user instruction to an existing persistent agent (executor or " +
			"sub-coordinator) as its next user message. The agent resumes its loop in the " +
			"background.\n" +
			"You can ONLY route to agents you spawned directly. To reach a deeper descendant, " +
			"route through the intermediate coordinator. Only valid for persistent lifetime — " +
			"oneshot tasks reject routes.\n" +
			"content: a plain string, or an array of content blocks " +
			'[{type:"text",text:"..."},{type:"image",data:"<base64>",mimeType:"..."}] ' +
			"to include images. Provide all necessary context — the agent doesn't see your conversation.",
		parameters: RouteParams,
		async execute(
			_id: string,
			params: Static<typeof RouteParams>,
		): Promise<AgentToolResult<{ id: string }>> {
			const content = params.content as UserContent;
			await registry.routeFromCaller(ctx.parentSessionKey, params.id, content);
			return {
				content: [{ type: "text", text: `routed content to ${params.id}` }],
				details: { id: params.id },
			};
		},
	};
}

function listAgentsTool(
	registry: WorkerRegistry,
	ctx: CoordinatorCtx,
): AgentTool<typeof ListParams, { count: number }> {
	return {
		name: "list_agents",
		label: "List Agents",
		description:
			"List agents directly under THIS coordinator, newest first. Shows id, kind, " +
			"lifetime, name, status, age. Use to find an existing agent before deciding " +
			"between route_to_agent and spawn_*.",
		parameters: ListParams,
		async execute(): Promise<AgentToolResult<{ count: number }>> {
			const rows = await registry.listForParent(ctx.parentSessionKey, 20);
			if (rows.length === 0) {
				return {
					content: [{ type: "text", text: "(no agents)" }],
					details: { count: 0 },
				};
			}
			const now = Date.now();
			const lines = rows.map(
				(r) =>
					`${r.id}  ${r.kind.padEnd(11)} ${r.lifetime.padEnd(10)} ${r.status.padEnd(10)} ${r.name}  (age=${formatAge(now - r.createdAt)}, turns=${r.turnCount}${r.error ? `, err=${truncate(r.error, 60)}` : ""})`,
			);
			return {
				content: [{ type: "text", text: lines.join("\n") }],
				details: { count: rows.length },
			};
		},
	};
}

function getAgentTool(
	registry: WorkerRegistry,
): AgentTool<typeof IdParams, { row: WorkerRow | null }> {
	return {
		name: "get_agent",
		label: "Get Agent",
		description:
			"Fetch one agent's status, system prompt, and transcript tail. Use to inspect " +
			"an agent's state without disturbing it.",
		parameters: IdParams,
		async execute(
			_id: string,
			params: Static<typeof IdParams>,
		): Promise<AgentToolResult<{ row: WorkerRow | null }>> {
			const row = await registry.get(params.id);
			if (!row) {
				return {
					content: [{ type: "text", text: `no agent with id=${params.id}` }],
					details: { row: null },
				};
			}
			const tail = extractTail(row.messagesJson);
			const lines = [
				`id=${row.id}`,
				`kind=${row.kind}`,
				`lifetime=${row.lifetime}`,
				`name=${row.name}`,
				`status=${row.status}`,
				`turns=${row.turnCount}`,
				`send_policy=${row.sendPolicy}`,
				`system_prompt=${truncate(row.systemPrompt, 200)}`,
				row.error ? `error=${row.error}` : "",
				"",
				"--- transcript tail ---",
				tail,
			].filter(Boolean);
			return {
				content: [{ type: "text", text: lines.join("\n") }],
				details: { row },
			};
		},
	};
}

function cancelAgentTool(
	registry: WorkerRegistry,
): AgentTool<typeof IdParams, { id: string }> {
	return {
		name: "cancel_agent",
		label: "Cancel Agent",
		description:
			"Cancel an agent. Aborts its run, marks the row cancelled, and tears down its " +
			"sandbox (executors only — coordinators have no container). Use to free " +
			"resources when an agent is no longer needed. NOTE: cancelling a coordinator " +
			"does not auto-cancel its children — cancel them explicitly first if needed.",
		parameters: IdParams,
		async execute(
			_id: string,
			params: Static<typeof IdParams>,
		): Promise<AgentToolResult<{ id: string }>> {
			await registry.cancel(params.id);
			return {
				content: [{ type: "text", text: `cancel requested for ${params.id}` }],
				details: { id: params.id },
			};
		},
	};
}

// ── Helpers ───────────────────────────────────────────────────────────

function formatAge(ms: number): string {
	const s = Math.floor(ms / 1000);
	if (s < 60) return `${s}s`;
	const m = Math.floor(s / 60);
	if (m < 60) return `${m}m`;
	const h = Math.floor(m / 60);
	if (h < 24) return `${h}h`;
	return `${Math.floor(h / 24)}d`;
}

function truncate(s: string, n: number): string {
	return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

function extractTail(messagesJson: string): string {
	try {
		const msgs = JSON.parse(messagesJson) as Array<{ role?: string; content?: unknown }>;
		if (!Array.isArray(msgs) || msgs.length === 0) return "(empty)";
		const tail = msgs.slice(-3);
		return tail
			.map((m) => {
				const role = m.role ?? "?";
				let text = "";
				if (typeof m.content === "string") text = m.content;
				else if (Array.isArray(m.content)) {
					for (const block of m.content) {
						if (block && typeof block === "object" && "type" in block) {
							if ((block as any).type === "text") text += String((block as any).text ?? "");
							else if ((block as any).type === "toolCall")
								text += `[tool:${(block as any).name ?? "?"}]`;
						}
					}
				}
				return `${role}: ${truncate(text, 300)}`;
			})
			.join("\n");
	} catch {
		return "(unparseable)";
	}
}
