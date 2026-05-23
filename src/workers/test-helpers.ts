import type { WorkerRow, WorkerKind, WorkerLifetime, WorkerStatus, SendPolicy } from "./store.ts";

export function fakeRow(over: Partial<WorkerRow> = {}): WorkerRow {
	return {
		id: "w-1",
		kind: "executor" as WorkerKind,
		lifetime: "persistent" as WorkerLifetime,
		parentSessionKey: "telegram:-1",
		sourceChannel: "telegram",
		sourceChatId: "-1",
		sourceReplyTo: null,
		name: "test-worker",
		initialPrompt: null,
		systemPrompt: "test system prompt",
		modelProvider: "openrouter",
		modelId: "openai/gpt-4o",
		status: "idle" as WorkerStatus,
		messagesJson: "[]",
		tapeId: null,
		containerName: "nexal-worker-test",
		createdAt: Date.now(),
		startedAt: null,
		updatedAt: Date.now(),
		completedAt: null,
		error: null,
		turnCount: 0,
		sendPolicy: "explicit" as SendPolicy,
		...over,
	};
}
