import { Type, type Static } from "@mariozechner/pi-ai";

import type { WorkerAgent } from "../workers/agent.ts";
import type { UserContent } from "../content.ts";

export const FileAttachmentSchema = Type.Object({
	path: Type.String({ description: "Absolute file path inside this agent's sandbox, e.g. /workspace/cat.jpg." }),
	mimeType: Type.Optional(Type.String({ description: 'MIME type, e.g. "image/jpeg". Inferred from the path when omitted.' })),
});

export const FileAttachmentsSchema = Type.Optional(Type.Array(FileAttachmentSchema, {
	description: "Files to attach by sandbox path. Prefer this over base64 image data.",
}));

export type FileAttachmentParam = Static<typeof FileAttachmentSchema>;

export async function attachSandboxFiles(
	runner: WorkerAgent,
	content: UserContent,
	files?: FileAttachmentParam[],
): Promise<UserContent> {
	if (!files || files.length === 0) return content;
	const client = runner.execClient;
	if (!client) {
		throw new Error("file attachments require an executor sandbox");
	}

	const blocks = typeof content === "string"
		? [{ type: "text" as const, text: content }]
		: [...content];
	for (const file of files) {
		const data = await client.readFile(file.path);
		blocks.push({
			type: "image",
			data: Buffer.from(data).toString("base64"),
			mimeType: file.mimeType ?? inferMimeType(file.path),
		});
	}
	return blocks;
}

function inferMimeType(path: string): string {
	const ext = path.split(".").pop()?.toLowerCase();
	switch (ext) {
		case "jpg":
		case "jpeg":
			return "image/jpeg";
		case "png":
			return "image/png";
		case "gif":
			return "image/gif";
		case "webp":
			return "image/webp";
		case "svg":
			return "image/svg+xml";
		default:
			return "application/octet-stream";
	}
}
