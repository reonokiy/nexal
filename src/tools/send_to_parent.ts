/**
 * send_to_parent — the only way for a spawned agent to send a
 * message UPWARDS in the tree.
 *
 * Tree edges:
 *   - parent → child: `send_to_agent` (only direct children)
 *   - child  → parent: `send_to_parent` (this tool)
 *   - child  → sibling: forbidden — must go through the common parent
 *
 * The destination is decided by the agent's `parent_session_key`:
 *   - looks like `"<channel>:<chatId>"` (contains `:`) → top-level
 *     coordinator: the registry calls `forwardToCoordinator`, which the
 *     entry point wires to `AgentPool.forwardChildReport`.
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
import type { WorkerAgent } from "../workers/agent.ts";
import { TextOnlyUserContentSchema, type UserContent, contentLength } from "../content.ts";
import { attachSandboxFiles, FileAttachmentsSchema } from "./file_content.ts";

const SendToParentParams = Type.Object({
	content: TextOnlyUserContentSchema,
	files: FileAttachmentsSchema,
});

export function createSendToParentTool(
	registry: WorkerRegistry,
	runner: WorkerAgent,
): AgentTool<typeof SendToParentParams, { bytes: number }> {
	return {
		name: "send_to_parent",
		label: "Send To Parent",
		description:
			"Send a message to the agent that spawned you (your parent in the tree). " +
			"This is the ONLY upward edge — you cannot talk to siblings or your parent's " +
			"parent directly. If you need a sibling to act, ask your parent to send the " +
			"work over.\n" +
			"content: a plain string, or an array of text content blocks. " +
			"If you need to include images or other downloaded files, put their sandbox paths in " +
			'files, e.g. files:[{path:"/workspace/cat.jpg",mimeType:"image/jpeg"}]. ' +
			"Do NOT paste base64 image data into content. Be self-contained — your parent doesn't see your transcript.",
		parameters: SendToParentParams,
		async execute(
			_id: string,
			params: Static<typeof SendToParentParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			const content = await attachSandboxFiles(runner, params.content as UserContent, params.files);
			await registry.sendToParent(runner.id, content);
			const len = contentLength(content);
			return {
				content: [{ type: "text", text: "[reported]" }],
				details: { bytes: len },
			};
		},
	};
}
