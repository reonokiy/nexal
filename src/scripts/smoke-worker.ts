/**
 * End-to-end smoke of the worker subsystem against a live nexal-gateway.
 *
 * Spawns one shot executor through the gateway, asks it to write a
 * file inside /workspace and call send_update, then asserts the stub
 * channel saw the message and the row ended up `completed`.
 *
 * Requirements:
 *   - nexal-gateway running and reachable
 *   - OPENROUTER_API_KEY (or change NEXAL_MODEL_PROVIDER/NEXAL_MODEL)
 *   - the gateway's [defaults].agent_bin pointing at target/release/nexal-agent
 *   - podman + sandbox image available to the gateway
 *
 * Env knobs:
 *   NEXAL_GATEWAY_URL    (default ws://127.0.0.1:5500)
 *   NEXAL_GATEWAY_TOKEN  (REQUIRED; matches the gateway's token)
 *   NEXAL_MODEL_PROVIDER (default openrouter)
 *   NEXAL_MODEL          (default openai/gpt-4o-mini)
 */
import { getModel } from "@mariozechner/pi-ai";

import type { Channel, OutgoingReply } from "../channels/types.ts";
import { GatewayClient } from "../gateway/client.ts";
import { createBashTool } from "../tools/bash.ts";
import { createSendUpdateTool } from "../tools/send_update.ts";
import { WorkerRegistry } from "../workers/registry.ts";
import type { WorkerAgent } from "../workers/agent.ts";
import { createWorkerStore } from "../workers/store.ts";

const GATEWAY_URL = process.env.NEXAL_GATEWAY_URL ?? "ws://127.0.0.1:5500";
const GATEWAY_TOKEN = process.env.NEXAL_GATEWAY_TOKEN;
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
	if (!GATEWAY_TOKEN) {
		throw new Error("NEXAL_GATEWAY_TOKEN env var is required");
	}
	const dbUrl = process.env.NEXAL_WORKERS_URL;
	if (!dbUrl) throw new Error("NEXAL_WORKERS_URL env var required (postgres connection string)");
	const store = await createWorkerStore({ url: dbUrl });
	console.log(`[smoke] gateway=${GATEWAY_URL}`);

	const gateway = new GatewayClient({
		url: GATEWAY_URL,
		token: GATEWAY_TOKEN,
		clientName: "smoke-worker",
	});
	await gateway.hello();

	const stub = new StubChannel();
	const channels = new Map<string, Channel>([["stub", stub]]);
	const model = getModel(PROVIDER as any, MODEL_ID);

	const registry = new WorkerRegistry({
		store,
		gateway,
		model,
		modelProvider: PROVIDER,
		modelId: MODEL_ID,
		channels,
		maxConcurrent: 1,
		executorSystemPromptDefault:
			"You are a test executor. Do exactly what the user asks using bash, then call send_update with a short confirmation.",
		coordinatorSystemPromptDefault: "You are a test coordinator (unused in this smoke).",
		executorTools: (runner: WorkerAgent) => {
			const client = runner.execClient;
			if (!client) return [createSendUpdateTool(runner)];
			return [createBashTool(client), createSendUpdateTool(runner)];
		},
		coordinatorTools: () => [], // unused
	});

	const row = await registry.spawn({
		kind: "executor",
		lifetime: "oneshot",
		parentSessionKey: "stub:smoke",
		sourceChannel: "stub",
		sourceChatId: "smoke-chat",
		name: "smoke",
		initialPrompt:
			'Create the file /workspace/out containing "hello-from-sub-agent", then call send_update with the text "done".',
		sendPolicy: "explicit",
	});
	console.log(`[smoke] spawned worker ${row.id}`);

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

	await registry.shutdown();
	await gateway.releaseAllAgents();
	await gateway.close();
	console.log("[smoke] OK");
}

main().catch((err) => {
	console.error("[smoke] FAIL", err);
	process.exit(1);
});
