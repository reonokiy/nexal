import { describe, expect, test } from "bun:test";

import { messagesToJson, jsonToMessages, messagesToEntries, entriesToMessages } from "../tape/index.ts";

describe("messagesToJson / jsonToMessages", () => {
	test("empty array round-trips", () => {
		const s = messagesToJson([]);
		expect(s).toBe("[]");
		expect(jsonToMessages(s)).toEqual([]);
	});

	test("empty string deserializes to empty array", () => {
		expect(jsonToMessages("")).toEqual([]);
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
		const decoded = jsonToMessages(messagesToJson(msgs));
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
		const decoded = jsonToMessages(messagesToJson(original));
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
		const decoded = jsonToMessages(messagesToJson(msgs));
		const parts = (decoded[0] as any).content;
		expect([...parts[0].data]).toEqual([...a]);
		expect(parts[1]).toEqual({ type: "text", text: "between" });
		expect([...parts[2].data]).toEqual([...b]);
	});

	test("bytes with zero length round-trip", () => {
		const bytes = new Uint8Array(0);
		const decoded = jsonToMessages(
			messagesToJson([
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
		expect(jsonToMessages(messagesToJson(msgs))).toEqual(msgs);
	});

	test("bytes marker key does not collide with unrelated objects", () => {
		const msgs = [
			{ role: "user", content: "ok", timestamp: 1, metadata: { __nexal_bytes_b64__: 123 } },
		] as any;
		const decoded = jsonToMessages(messagesToJson(msgs));
		expect((decoded[0] as any).metadata).toEqual({ __nexal_bytes_b64__: 123 });
	});

	test("messagesToEntries preserves message metadata for tape replay/audit", () => {
		const entries = messagesToEntries([
			{
				role: "user",
				content: "resume work",
				timestamp: 1,
				meta: { internal: true, promptKind: "resume" },
			},
		] as any);

		expect(entries[0]!.meta).toEqual({ internal: true, promptKind: "resume" });
	});

	test("entriesToMessages restores tape metadata onto messages", () => {
		const messages = entriesToMessages([
			{
				id: 1,
				kind: "message",
				payload: { role: "user", content: "resume work", timestamp: 1 },
				meta: { internal: true, promptKind: "resume" },
				date: "2026-05-28T00:00:00.000Z",
			},
		]);

		expect((messages[0] as any).meta).toEqual({ internal: true, promptKind: "resume" });
	});

	test("messagesToEntries normalizes string assistant content into text blocks", () => {
		const entries = messagesToEntries([
			{ role: "assistant", content: "done", timestamp: 1 },
		] as any);

		expect(entries[0]!.payload.content).toEqual([{ type: "text", text: "done" }]);
	});
});
