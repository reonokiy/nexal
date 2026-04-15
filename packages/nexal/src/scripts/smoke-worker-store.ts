/**
 * Smoke the WorkerStore CRUD without touching podman or an LLM.
 *
 * Defaults to a throw-away sqlite file under /tmp. To smoke the
 * postgres path, export NEXAL_WORKERS_PG_URL=postgres://... and pass
 * `pg` as the first arg:
 *
 *   bun run src/scripts/smoke-worker-store.ts          # sqlite (default)
 *   bun run src/scripts/smoke-worker-store.ts pg       # postgres
 */
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
	deserializeMessages,
	serializeMessages,
} from "../workers/serialize.ts";
import { createWorkerStore } from "../workers/store.ts";

const mode = (process.argv[2] ?? "sqlite").toLowerCase();

function assertEq<T>(actual: T, expected: T, label: string): void {
	if (JSON.stringify(actual) !== JSON.stringify(expected)) {
		throw new Error(
			`[assert] ${label}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`,
		);
	}
}

async function main(): Promise<void> {
	let url: string;
	let backend: "sqlite" | "postgres";
	let cleanup: (() => Promise<void>) | null = null;

	if (mode === "pg" || mode === "postgres") {
		backend = "postgres";
		url = process.env.NEXAL_WORKERS_PG_URL ?? "";
		if (!url) throw new Error("NEXAL_WORKERS_PG_URL must be set for postgres smoke");
	} else {
		backend = "sqlite";
		const dir = await mkdtemp(join(tmpdir(), "nexal-smoke-"));
		url = join(dir, "workers.db");
		cleanup = async () => {
			await rm(dir, { recursive: true, force: true });
		};
	}

	console.log(`[smoke] backend=${backend} url=${url}`);
	const store = await createWorkerStore({ backend, url });

	// insert persistent executor + get
	const persistent = await store.insert({
		id: "00000000-0000-0000-0000-000000000001",
		kind: "executor",
		lifetime: "persistent",
		parentSessionKey: "telegram:-1001",
		sourceChannel: "telegram",
		sourceChatId: "-1001",
		name: "smoke-persistent",
		initialPrompt: null,
		systemPrompt: "you are a smoke executor",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		containerName: "nexal-worker-smoke1",
		sendPolicy: "explicit",
	});
	console.log("[smoke] inserted", persistent.id, persistent.status);
	assertEq(persistent.status, "spawning", "initial status");
	assertEq(persistent.kind, "executor", "kind");
	assertEq(persistent.lifetime, "persistent", "lifetime");

	const fetched = await store.get(persistent.id);
	assertEq(fetched?.name, "smoke-persistent", "get name");

	// markStarted → setMessages → markIdle (persistent goes idle, not completed)
	await store.markStarted(persistent.id);
	const started = await store.get(persistent.id);
	assertEq(started?.status, "running", "markStarted");

	const msgs = serializeMessages([
		{ role: "user", content: "hi", timestamp: 1 },
		{
			role: "assistant",
			content: [{ type: "text", text: "hello" }],
			timestamp: 2,
			stopReason: "complete",
		},
	] as any);
	await store.setMessages(persistent.id, msgs, 1);
	const mid = await store.get(persistent.id);
	assertEq(mid?.turnCount, 1, "turn count");
	assertEq(deserializeMessages(mid!.messagesJson).length, 2, "round-trip length");

	await store.markIdle(persistent.id, msgs);
	const idle = await store.get(persistent.id);
	assertEq(idle?.status, "idle", "markIdle");

	// shot executor → markCompleted
	const shot = await store.insert({
		id: "00000000-0000-0000-0000-000000000002",
		kind: "executor",
		lifetime: "shot",
		parentSessionKey: "telegram:-1001",
		sourceChannel: "telegram",
		sourceChatId: "-1001",
		name: "smoke-shot",
		initialPrompt: "do the one thing",
		systemPrompt: "shot executor",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		containerName: "nexal-worker-smoke2",
	});
	assertEq(shot.lifetime, "shot", "shot lifetime");
	await store.markStarted(shot.id);
	await store.markCompleted(shot.id, msgs);
	const done = await store.get(shot.id);
	assertEq(done?.status, "completed", "markCompleted");
	if (!done?.completedAt) throw new Error("completedAt not set");

	// sub-coordinator (persistent, no bash)
	const coord = await store.insert({
		id: "00000000-0000-0000-0000-000000000003",
		kind: "coordinator",
		lifetime: "persistent",
		parentSessionKey: "telegram:-1001",
		sourceChannel: "telegram",
		sourceChatId: "-1001",
		name: "smoke-coord",
		initialPrompt: null,
		systemPrompt: "sub-coordinator",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		containerName: "nexal-worker-smoke3",
	});
	assertEq(coord.kind, "coordinator", "coord kind");
	assertEq(coord.lifetime, "persistent", "coord lifetime");

	// agents spawned UNDER the sub-coordinator have its id as parent
	await store.insert({
		id: "00000000-0000-0000-0000-000000000004",
		kind: "executor",
		lifetime: "shot",
		parentSessionKey: coord.id,
		sourceChannel: "telegram",
		sourceChatId: "-1001",
		name: "smoke-coord-child",
		initialPrompt: "x",
		systemPrompt: "y",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		containerName: "nexal-worker-smoke4",
	});
	const childList = await store.listByParent(coord.id, 50);
	assertEq(childList.length, 1, "coord subtree size");

	// byte round-trip
	const bytes = new Uint8Array([0xde, 0xad, 0xbe, 0xef]);
	const withImage = serializeMessages([
		{
			role: "user",
			content: [{ type: "image", data: bytes, mimeType: "image/png" }],
			timestamp: 3,
		},
	] as any);
	const back = deserializeMessages(withImage);
	const img = (back[0] as any).content[0] as { data: Uint8Array };
	if (!(img.data instanceof Uint8Array)) throw new Error("bytes did not round-trip");
	assertEq([...img.data], [...bytes], "bytes round-trip");

	// listByStatus / listByParent
	const idleList = await store.listByStatus("idle");
	assertEq(idleList.length >= 1, true, "idle list non-empty");
	const parent = await store.listByParent("telegram:-1001", 50);
	assertEq(parent.length, 3, "listByParent count (top-level only)");

	// markFailed
	await store.markFailed(persistent.id, "smoke failure");
	const failed = await store.get(persistent.id);
	assertEq(failed?.status, "failed", "markFailed");
	assertEq(failed?.error, "smoke failure", "error text");

	await store.close();
	if (cleanup) await cleanup();
	console.log("[smoke] OK");
}

main().catch((err) => {
	console.error("[smoke] FAIL", err);
	process.exit(1);
});
