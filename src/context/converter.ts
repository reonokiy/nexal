import type { AgentMessage } from "@mariozechner/pi-agent-core";

// Re-export tape serialization for backward compat
export { entriesToMessages, messagesToEntries, truncateMessages } from "../tape/serialize.ts";

const BYTES_MARKER = "__nexal_bytes_b64__";

/** Serialize AgentMessages to JSON, handling Uint8Array via base64. */
export function messagesToJson(messages: AgentMessage[]): string {
	return JSON.stringify(messages, (_key, value) => {
		if (value instanceof Uint8Array) {
			return { [BYTES_MARKER]: Buffer.from(value).toString("base64") };
		}
		return value;
	});
}

/** Deserialize JSON back to AgentMessages, restoring base64-encoded Uint8Arrays. */
export function jsonToMessages(json: string): AgentMessage[] {
	if (!json || json === "[]") return [];
	return JSON.parse(json, (_key, value) => {
		if (value && typeof value === "object" && typeof (value as any)[BYTES_MARKER] === "string") {
			return new Uint8Array(Buffer.from((value as any)[BYTES_MARKER], "base64"));
		}
		return value;
	}) as AgentMessage[];
}
