/**
 * AgentMessage JSON (de)serialization.
 *
 * `AgentMessage` is a discriminated union of user / assistant / toolResult
 * / custom messages. The only non-JSON-safe values are `Uint8Array`
 * bytes inside image content blocks — we base64-encode them on write
 * and decode back to `Uint8Array` on read.
 *
 * This is driver-agnostic (identical for sqlite and postgres backends).
 */
import type { AgentMessage } from "@mariozechner/pi-agent-core";

const BYTES_MARKER = "__nexal_bytes_b64__";

export function serializeMessages(msgs: AgentMessage[]): string {
	return JSON.stringify(msgs, (_key, value) => {
		if (value instanceof Uint8Array) {
			return { [BYTES_MARKER]: Buffer.from(value).toString("base64") };
		}
		return value;
	});
}

export function deserializeMessages(s: string): AgentMessage[] {
	if (!s || s === "[]") return [];
	return JSON.parse(s, (_key, value) => {
		if (value && typeof value === "object" && typeof (value as any)[BYTES_MARKER] === "string") {
			return new Uint8Array(Buffer.from((value as any)[BYTES_MARKER], "base64"));
		}
		return value;
	}) as AgentMessage[];
}
