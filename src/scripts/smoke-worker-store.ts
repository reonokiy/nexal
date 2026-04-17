/**
 * Smoke the WorkerStore CRUD against a real Postgres.
 *
 *   NEXAL_WORKERS_URL=postgres://user:pw@host:5432/db \
 *     bun run src/scripts/smoke-worker-store.ts
 *
 * The smoke uses random UUIDs so successive runs don't collide on the
 * primary key. Existing rows are left alone.
 */
import {
	deserializeMessages,
	serializeMessages,
} from "../workers/serialize.ts";
import { createWorkerStore } from "../workers/store.ts";

const URL = process.env.NEXAL_WORKERS_URL;
if (!URL) throw new Error("NEXAL_WORKERS_URL env var required (postgres connection string)");

function assertEq<T>(actual: T, expected: T, label: string): void {
	if (JSON.stringify(actual) !== JSON.stringify(expected)) {
		throw new Error(
			`[assert] ${label}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`,
		);
	}
}

async function main(): Promise<void> {
	console.log(`[smoke] url=${URL?.replace(/:[^@/]+@/, ":***@")}`);
	const store = await createWorkerStore({ url: URL! });

	const parentKey = `telegram:smoke-${crypto.randomUUID()}`;

	// insert persistent executor + get
	const persistent = await store.insert({
		id: crypto.randomUUID(),
		kind: "executor",
		lifetime: "persistent",
		parentSessionKey: parentKey,
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

	// shot executor
	const shot = await store.insert({
		id: crypto.randomUUID(),
		kind: "executor",
		lifetime: "oneshot",
		parentSessionKey: parentKey,
		sourceChannel: "telegram",
		sourceChatId: "-1001",
		name: "smoke-shot",
		initialPrompt: "do the one thing",
		systemPrompt: "shot executor",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		containerName: "nexal-worker-smoke2",
	});
	assertEq(shot.lifetime, "oneshot", "shot lifetime");
	await store.markStarted(shot.id);
	await store.markCompleted(shot.id, msgs);
	const done = await store.get(shot.id);
	assertEq(done?.status, "completed", "markCompleted");
	if (!done?.completedAt) throw new Error("completedAt not set");

	// sub-coordinator + child
	const coord = await store.insert({
		id: crypto.randomUUID(),
		kind: "coordinator",
		lifetime: "persistent",
		parentSessionKey: parentKey,
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

	await store.insert({
		id: crypto.randomUUID(),
		kind: "executor",
		lifetime: "oneshot",
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

	const idleList = await store.listByStatus("idle");
	assertEq(idleList.length >= 1, true, "idle list non-empty");
	const parent = await store.listByParent(parentKey, 50);
	assertEq(parent.length, 3, "listByParent count (top-level only)");

	// markFailed
	await store.markFailed(persistent.id, "smoke failure");
	const failed = await store.get(persistent.id);
	assertEq(failed?.status, "failed", "markFailed");
	assertEq(failed?.error, "smoke failure", "error text");

	await store.close();
	console.log("[smoke] OK");
}

main().catch((err) => {
	console.error("[smoke] FAIL", err);
	process.exit(1);
});
