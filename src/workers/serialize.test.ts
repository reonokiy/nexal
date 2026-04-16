import { describe, expect, test } from "bun:test";

import { deserializeMessages, serializeMessages } from "./serialize.ts";

describe("serializeMessages / deserializeMessages", () => {
	test("empty array round-trips", () => {
		const s = serializeMessages([]);
		expect(s).toBe("[]");
		expect(deserializeMessages(s)).toEqual([]);
	});

	test("empty string deserializes to empty array", () => {
		expect(deserializeMessages("")).toEqual([]);
	});

	test("plain user+assistant messages round-trip", () => {
		const msgs = [
			{ role: "user", content: "hi", timestamp: 1 },
			{
				role: "assistant",
				content: [{ type: "text", text: "hello" }],
				timestamp: 2,
				stopReason: "complete",
			},
		] as any;
		const decoded = deserializeMessages(serializeMessages(msgs));
		expect(decoded).toEqual(msgs);
	});

	test("Uint8Array image content round-trips through base64", () => {
		const bytes = new Uint8Array([0xde, 0xad, 0xbe, 0xef, 0x00, 0xff]);
		const original = [
			{
				role: "user",
				content: [{ type: "image", data: bytes, mimeType: "image/png" }],
				timestamp: 3,
			},
		] as any;
		const decoded = deserializeMessages(serializeMessages(original));
		const img = (decoded[0] as any).content[0];
		expect(img.data).toBeInstanceOf(Uint8Array);
		expect([...img.data]).toEqual([...bytes]);
		expect(img.mimeType).toBe("image/png");
	});

	test("nested bytes in arrays round-trip", () => {
		const a = new Uint8Array([1, 2, 3]);
		const b = new Uint8Array([4, 5]);
		const msgs = [
			{
				role: "user",
				content: [
					{ type: "image", data: a, mimeType: "image/jpeg" },
					{ type: "text", text: "between" },
					{ type: "image", data: b, mimeType: "image/png" },
				],
				timestamp: 1,
			},
		] as any;
		const decoded = deserializeMessages(serializeMessages(msgs));
		const parts = (decoded[0] as any).content;
		expect([...parts[0].data]).toEqual([...a]);
		expect(parts[1]).toEqual({ type: "text", text: "between" });
		expect([...parts[2].data]).toEqual([...b]);
	});

	test("bytes with zero length round-trip", () => {
		const bytes = new Uint8Array(0);
		const decoded = deserializeMessages(
			serializeMessages([
				{ role: "user", content: [{ type: "image", data: bytes, mimeType: "image/png" }], timestamp: 1 },
			] as any),
		);
		const img = (decoded[0] as any).content[0];
		expect(img.data).toBeInstanceOf(Uint8Array);
		expect(img.data.length).toBe(0);
	});

	test("non-byte objects pass through unchanged", () => {
		const msgs = [
			{
				role: "user",
				content: [{ type: "text", text: "no bytes here", meta: { nested: true } }],
				timestamp: 1,
			},
		] as any;
		expect(deserializeMessages(serializeMessages(msgs))).toEqual(msgs);
	});

	test("bytes marker key does not collide with unrelated objects", () => {
		// A regular object happening to have a similar key shouldn't be confused.
		const msgs = [
			{ role: "user", content: "ok", timestamp: 1, metadata: { __nexal_bytes_b64__: 123 } },
		] as any;
		const decoded = deserializeMessages(serializeMessages(msgs));
		// The marker check requires a STRING value — 123 (number) means
		// it stays a plain object, not a Uint8Array.
		expect((decoded[0] as any).metadata).toEqual({ __nexal_bytes_b64__: 123 });
	});
});
