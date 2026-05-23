import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static } from "@mariozechner/pi-ai";

import type { WorkerRegistry } from "../../workers/registry.ts";
import type { WorkerRow } from "../../workers/store.ts";
import type { UserContent } from "../../content.ts";

import { formatAge, truncate } from "../../utils/format.ts";

import type { CoordinatorCtx } from "./schemas.ts";
import { IdParams, ListParams, RouteParams } from "./schemas.ts";

export function routeToAgentTool(
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

export function listAgentsTool(
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

export function getAgentTool(
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

export function cancelAgentTool(
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
