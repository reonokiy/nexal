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
 *     `route(parentId, message)` to inject as the parent's next user
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

const ReportParams = Type.Object({
	text: Type.String({
		description:
			"Message to deliver to your parent. It arrives as your parent's next user " +
			"message. Be self-contained — your parent doesn't see your transcript.",
	}),
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
			"work over.",
		parameters: ReportParams,
		async execute(
			_id: string,
			params: Static<typeof ReportParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			await registry.reportToParent(runner.id, params.text);
			return {
				content: [{ type: "text", text: "[reported]" }],
				details: { bytes: params.text.length },
			};
		},
	};
}
