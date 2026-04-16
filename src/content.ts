/**
 * Shared content type utilities — aligns with pi-ai's `UserMessage.content`.
 *
 * pi-ai defines `UserMessage.content` as `string | (TextContent | ImageContent)[]`.
 * This module re-exports that shape as `UserContent` and provides:
 *   - conversion helpers between channel-layer `ImageAttachment` and `UserContent`
 *   - a TypeBox schema (`UserContentSchema`) for tool parameters
 */
import type { ImageContent, TextContent, UserMessage } from "@mariozechner/pi-ai";
import { Type, type TSchema } from "@mariozechner/pi-ai";

import type { ImageAttachment } from "./channels/types.ts";

// ── Type alias ───────────────────────────────────��───────────────────

/** Same shape as `UserMessage["content"]` from pi-ai. */
export type UserContent = UserMessage["content"];

// ── TypeBox schemas (for tool parameters) ────────────────────────────

export const ImageContentSchema = Type.Object({
	type: Type.Literal("image"),
	data: Type.String({ description: "Base64-encoded image bytes." }),
	mimeType: Type.String({ description: 'MIME type, e.g. "image/jpeg".' }),
});

export const TextContentSchema = Type.Object({
	type: Type.Literal("text"),
	text: Type.String(),
});

/** Tool-parameter schema matching `UserMessage["content"]`. */
export const UserContentSchema = Type.Union([
	Type.String({ description: "Plain text message." }),
	Type.Array(Type.Union([TextContentSchema, ImageContentSchema]), {
		description:
			"Array of content blocks. Use when sending images alongside text.",
	}),
]) as TSchema;

// ── Conversion helpers ───────────────────────────────────────────────

/** Channel-layer `ImageAttachment` → pi-ai `ImageContent`. */
export function attachmentToImageContent(att: ImageAttachment): ImageContent {
	const data =
		att.data instanceof Uint8Array
			? Buffer.from(att.data).toString("base64")
			: att.data;
	return { type: "image", data, mimeType: att.mimeType };
}

/** Build `UserContent` from text + optional channel-layer images. */
export function buildUserContent(
	text: string,
	images?: ImageAttachment[],
): UserContent {
	if (!images || images.length === 0) return text;
	return [
		{ type: "text", text } as TextContent,
		...images.map(attachmentToImageContent),
	];
}

/** Extract plain text from `UserContent` (drops image blocks). */
export function extractTextFromContent(content: UserContent): string {
	if (typeof content === "string") return content;
	return content
		.filter((b): b is TextContent => b.type === "text")
		.map((b) => b.text)
		.join("");
}

/** Extract `ImageContent[]` from `UserContent`. */
export function extractImagesFromContent(content: UserContent): ImageContent[] {
	if (typeof content === "string") return [];
	return content.filter((b): b is ImageContent => b.type === "image");
}

/** Convert `ImageContent` back to channel-layer `ImageAttachment`. */
export function imageContentToAttachment(img: ImageContent): ImageAttachment {
	return { data: img.data, mimeType: img.mimeType, filename: "image" };
}
