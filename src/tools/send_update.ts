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

import type { WorkerAgent } from "../workers/agent.ts";
import { UserContentSchema, type UserContent } from "../content.ts";

const SendUpdateParams = Type.Object({
	content: UserContentSchema,
});

export function createSendUpdateTool(
	runner: WorkerAgent,
): AgentTool<typeof SendUpdateParams, { bytes: number }> {
	return {
		name: "send_update",
		label: "Send Update",
		description:
			"Send a progress message to the user's chat. Use for milestones, questions, " +
			"or the final result. Each call is one chat message, so batch content and " +
			"avoid spamming every intermediate thought.\n" +
			"content: a plain string (lightweight Markdown works on Telegram), or an array of " +
			'content blocks [{type:"text",text:"..."},{type:"image",data:"<base64>",mimeType:"image/png"}] ' +
			"when you need to send images.",
		parameters: SendUpdateParams,
		async execute(
			_toolCallId: string,
			params: Static<typeof SendUpdateParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			const content = params.content as UserContent;
			await runner.sendToChat(content);
			const len = typeof content === "string" ? content.length : JSON.stringify(content).length;
			return {
				content: [{ type: "text", text: "[sent]" }],
				details: { bytes: len },
			};
		},
	};
}
