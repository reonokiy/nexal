/**
 * send_to_user — the sub-agent's only direct channel to the user.
 *
 * Under the default `explicit` send policy nothing the sub-agent says
 * reaches the chat unless it calls this tool. That forces clean
 * milestone reporting instead of streaming every LLM hop into the
 * Telegram thread.
 */
import type { AgentTool, AgentToolResult } from "@mariozechner/pi-agent-core";
import { type Static, Type } from "@mariozechner/pi-ai";

import type { WorkerAgent } from "../workers/agent.ts";
import { TextOnlyUserContentSchema, type UserContent, contentLength } from "../content.ts";
import { attachSandboxFiles, FileAttachmentsSchema } from "./file_content.ts";

const SendToUserParams = Type.Object({
	content: TextOnlyUserContentSchema,
	files: FileAttachmentsSchema,
});

export function createSendToUserTool(
	runner: WorkerAgent,
): AgentTool<typeof SendToUserParams, { bytes: number }> {
	return {
		name: "send_to_user",
		label: "Send To User",
		description:
			"Send a progress message to the user's chat. Use for milestones, questions, " +
			"or the final result. Each call is one chat message, so batch content and " +
			"avoid spamming every intermediate thought.\n" +
			"content: a plain string (lightweight Markdown works on Telegram), or an array of text blocks. " +
			"If you need to send images or downloaded files, put their sandbox paths in " +
			'files, e.g. files:[{path:"/workspace/cat.jpg",mimeType:"image/jpeg"}]. ' +
			"Do NOT paste base64 image data into content.",
		parameters: SendToUserParams,
		async execute(
			_toolCallId: string,
			params: Static<typeof SendToUserParams>,
		): Promise<AgentToolResult<{ bytes: number }>> {
			const content = await attachSandboxFiles(runner, params.content as UserContent, params.files);
			await runner.sendToChat(content);
			const len = contentLength(content);
			return {
				content: [{ type: "text", text: "[sent]" }],
				details: { bytes: len },
			};
		},
	};
}
