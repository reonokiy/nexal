/**
 * End-to-end smoke of the worker subsystem (executor only — see the
 * coordinator chain in actual Telegram use; smoking the recursive
 * coordinator dispatcher requires a real LLM playing the dispatcher
 * which is hard to make deterministic).
 *
 * Spawns one shot executor with its own Podman container + real LLM,
 * asks it to write a file inside /workspace and call send_update,
 * then asserts the stub channel saw the message and the row ended up
 * `completed`.
 *
 * Requirements:
 *   - podman on PATH + pull-access to the sandbox image
 *   - OPENROUTER_API_KEY (or change NEXAL_MODEL_PROVIDER/NEXAL_MODEL)
 *   - nexal-agent binary at target/release/nexal-agent
 *
 * Env knobs:
 *   NEXAL_SANDBOX_IMAGE   (default ghcr.io/reonokiy/nexal-sandbox:…)
 *   NEXAL_AGENT_BIN (default ../../../../target/release/nexal-agent)
 *   NEXAL_MODEL_PROVIDER  (default openrouter)
 *   NEXAL_MODEL           (default openai/gpt-4o-mini)
 */
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { getModel } from "@mariozechner/pi-ai";

import type { Channel, OutgoingReply } from "../channels/types.ts";
import { PodmanBackend } from "../sandbox/podman.ts";
import { createBashTool } from "../tools/bash.ts";
import { createSendUpdateTool } from "../tools/send_update.ts";
import { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerRunner } from "../workers/runner.ts";
import { createWorkerStore } from "../workers/store.ts";

const IMAGE =
	process.env.NEXAL_SANDBOX_IMAGE ?? "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13";
const AGENT_BIN =
	process.env.NEXAL_AGENT_BIN ??
	`${import.meta.dir}/../../../../target/release/nexal-agent`;
const PROVIDER = process.env.NEXAL_MODEL_PROVIDER ?? "openrouter";
const MODEL_ID = process.env.NEXAL_MODEL ?? "openai/gpt-4o-mini";

class StubChannel implements Channel {
	readonly name = "stub";
	readonly sent: OutgoingReply[] = [];
	async start(): Promise<void> {}
	async send(reply: OutgoingReply): Promise<void> {
		this.sent.push(reply);
		console.log(`[stub] chat=${reply.chatId} text=${reply.text}`);
	}
	async stop(): Promise<void> {}
}

async function main(): Promise<void> {
	const dir = await mkdtemp(join(tmpdir(), "nexal-smoke-worker-"));
	const workspaceDir = join(dir, "workspace");
	await Bun.write(join(workspaceDir, ".placeholder"), "").catch(() => undefined);

	const store = await createWorkerStore({
		backend: "sqlite",
		url: join(dir, "workers.db"),
	});
	console.log(`[smoke] db=${join(dir, "workers.db")} workspace=${workspaceDir}`);

	const sandbox = new PodmanBackend({
		image: IMAGE,
		agentBin: AGENT_BIN,
		memory: "512m",
		cpus: "1.0",
		pidsLimit: 256,
		network: false,
		workspace: workspaceDir,
	});
	const stub = new StubChannel();
	const channels = new Map<string, Channel>([["stub", stub]]);
	const model = getModel(PROVIDER as any, MODEL_ID);

	const registry = new WorkerRegistry({
		store,
		sandbox,
		model,
		modelProvider: PROVIDER,
		modelId: MODEL_ID,
		channels,
		maxConcurrent: 1,
		executorSystemPromptDefault:
			"You are a test executor. Do exactly what the user asks using bash, then call send_update with a short confirmation.",
		coordinatorSystemPromptDefault: "You are a test coordinator (unused in this smoke).",
		executorTools: (runner: WorkerRunner) => {
			const client = runner.execClient;
			if (!client) return [createSendUpdateTool(runner)];
			return [createBashTool(client), createSendUpdateTool(runner)];
		},
		coordinatorTools: () => [], // unused
	});

	const row = await registry.spawn({
		kind: "executor",
		lifetime: "shot",
		parentSessionKey: "stub:smoke",
		sourceChannel: "stub",
		sourceChatId: "smoke-chat",
		name: "smoke",
		initialPrompt:
			'Create the file /workspace/out containing "hello-from-sub-agent", then call send_update with the text "done".',
		sendPolicy: "explicit",
	});
	console.log(`[smoke] spawned worker ${row.id}`);

	// Poll for terminal state.
	const deadline = Date.now() + 120_000;
	let final = row;
	while (Date.now() < deadline) {
		await new Promise((r) => setTimeout(r, 1500));
		const cur = await store.get(row.id);
		if (!cur) continue;
		final = cur;
		console.log(`[smoke] status=${cur.status} turns=${cur.turnCount}`);
		if (cur.status === "completed" || cur.status === "failed" || cur.status === "cancelled") break;
	}

	if (final.status !== "completed") {
		throw new Error(`worker did not complete — status=${final.status} error=${final.error ?? ""}`);
	}

	const sawDone = stub.sent.some((r) => r.text.includes("done"));
	if (!sawDone) {
		throw new Error(`stub channel did not see a 'done' update: ${JSON.stringify(stub.sent)}`);
	}

	const out = Bun.file(join(workspaceDir, "out"));
	if (!(await out.exists())) {
		throw new Error(`executor did not create /workspace/out`);
	}
	const content = (await out.text()).trim();
	if (!content.includes("hello-from-sub-agent")) {
		throw new Error(`/workspace/out content unexpected: ${content}`);
	}

	await registry.shutdown();
	await sandbox.releaseAll();
	await rm(dir, { recursive: true, force: true });
	console.log("[smoke] OK");
}

main().catch((err) => {
	console.error("[smoke] FAIL", err);
	process.exit(1);
});
