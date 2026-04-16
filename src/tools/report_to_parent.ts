/**
 * report_to_parent — the only way for a spawned agent to send a
 * message UPWARDS in the tree.
 *
 * Tree edges:
 *   - parent → child: `route_to_agent` (only direct children)
 *   - child  → parent: `report_to_parent` (this tool)
 *   - child  → sibling: forbidden — must go through the common parent
 *
 * The destination is decided by the agent's `parent_session_key`:
 *   - looks like `"<channel>:<chatId>"` (contains `:`) → top-level
 *     coordinator: the registry calls `deliverToTopLevel`, which the
 *     entry point wires to `AgentPool.injectMessage`.
 *   - otherwise it's another worker's id → the registry calls its own
 *     `route(parentId, content)` to inject as the parent's next user
 *     message.
 *
 * Available to executors AND sub-coordinators. Coordinators use it to
 * escalate ("I can't decide this, please advise"); executors use it to
 * report results ("done, here's what I found") to a parent
 * sub-coordinator that needs to act on the result.
 */
import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static, Type } from "@mariozechner/pi-ai";

import type { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerRunner } from "../workers/runner.ts";
import { UserContentSchema, type UserContent } from "../content.ts";

const ReportParams = Type.Object({
	content: UserContentSchema,
});

export function createReportToParentTool(
	registry: WorkerRegistry,
	runner: WorkerRunner,
): AgentTool<typeof ReportParams, { bytes: number }> {
	return {
		name: "report_to_parent",
		label: "Report To Parent",
		description:
			"Send a message to the agent that spawned you (your parent in the tree). " +
			"This is the ONLY upward edge — you cannot talk to siblings or your parent's " +
			"parent directly. If you need a sibling to act, ask your parent to route the " +
			"work over.\n" +
			"content: a plain string, or an array of content blocks " +
			'[{type:"text",text:"..."},{type:"image",data:"<base64>",mimeType:"image/jpeg"}] ' +
			"when you need to include images. Be self-contained — your parent doesn't see your transcript.",
		parameters: ReportParams,
		async execute(
			_id: string,
			params: Static<typeof ReportParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			const content = params.content as UserContent;
			await registry.reportToParent(runner.id, content);
			const len = typeof content === "string" ? content.length : JSON.stringify(content).length;
			return {
				content: [{ type: "text", text: "[reported]" }],
				details: { bytes: len },
			};
		},
	};
}
