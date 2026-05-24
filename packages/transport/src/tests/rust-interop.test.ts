import { describe, expect, test } from "bun:test";
import { decodeFrame, encodeFrame } from "../codec.ts";

function hex(bytes: Uint8Array): string {
	return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function unhex(s: string): Uint8Array {
	const out = new Uint8Array(s.length / 2);
	for (let i = 0; i < out.length; i++) out[i] = Number.parseInt(s.slice(i * 2, i * 2 + 2), 16);
	return out;
}

async function runFixture(...args: string[]): Promise<string> {
	const proc = Bun.spawn(["cargo", "run", "-q", "-p", "nexal-utils-transport", "--bin", "transport-codec-fixture", "--", ...args], {
		cwd: new URL("../../..", import.meta.url).pathname,
		stderr: "pipe",
		stdout: "pipe",
	});
	const [stdout, stderr, code] = await Promise.all([
		new Response(proc.stdout).text(),
		new Response(proc.stderr).text(),
		proc.exited,
	]);
	if (code !== 0) throw new Error(`fixture failed (${code}): ${stderr}`);
	return stdout.trim();
}

describe("TS/Rust transport wire interop", () => {
	test("Rust decodes a TS-encoded request", async () => {
		const frame = encodeFrame({
			stream: "agent-1",
			id: "req-1",
			method: "process/start",
			params: {
				process_id: "p1",
				argv: ["bash", "-lc", "pwd"],
			},
		});

		const out = await runFixture("decode-request", hex(frame));
		const decoded = JSON.parse(out);
		expect(decoded).toMatchObject({
			kind: "request",
			stream: "agent-1",
			id: "req-1",
			method: "process/start",
		});
		expect(decoded.params.argv).toEqual(["bash", "-lc", "pwd"]);
	}, 30_000);

	test("TS decodes a Rust-encoded response", async () => {
		const out = await runFixture("encode-response");
		const decoded = decodeFrame(unhex(out));
		expect(decoded).toEqual({
			id: "req-1",
			result: { ok: true },
		});
	}, 30_000);
});
