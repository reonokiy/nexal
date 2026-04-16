/**
 * send_update — the sub-agent's only direct channel to the user.
 *
 * Under the default `explicit` send policy nothing the sub-agent says
 * reaches the chat unless it calls this tool. That forces clean
 * milestone reporting instead of streaming every LLM hop into the
 * Telegram thread.
 */
import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static, Type } from "@mariozechner/pi-ai";

import type { WorkerRunner } from "../workers/runner.ts";

const SendUpdateParams = Type.Object({
	text: Type.String({
		description:
			"Message body. Plain text (lightweight Markdown works on Telegram). " +
			"Keep concise — each call becomes a separate chat message.",
	}),
});

export function createSendUpdateTool(
	runner: WorkerRunner,
): AgentTool<typeof SendUpdateParams, { bytes: number }> {
	return {
		name: "send_update",
		label: "Send Update",
		description:
			"Send a progress message to the user's chat. Use for milestones, questions, " +
			"or the final result. Each call is one chat message, so batch content and " +
			"avoid spamming every intermediate thought.",
		parameters: SendUpdateParams,
		async execute(
			_toolCallId: string,
			params: Static<typeof SendUpdateParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			await runner.sendToSourceChat(params.text);
			return {
				content: [{ type: "text", text: "[sent]" }],
				details: { bytes: params.text.length },
			};
		},
	};
}
