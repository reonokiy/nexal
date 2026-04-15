/**
 * Postgres schema for the `workers` table (Drizzle).
 *
 * Two orthogonal axes:
 *
 *   `kind`     — what tools the agent has:
 *     - `"coordinator"` — dispatcher only (spawn_…, route_to_agent, …),
 *                        NO bash. Can spawn sub-coordinators.
 *     - `"executor"`    — bash + send_update. Does the actual work.
 *
 *   `lifetime` — when the agent dies:
 *     - `"persistent"` — stays alive across many turns; accepts routes.
 *                        Coordinators are always persistent.
 *     - `"shot"`       — terminates on `agent_end`. Only valid for
 *                        executors.
 *
 * Status set:
 *   - `spawning` — row created, container being acquired
 *   - `idle`     — persistent agent is alive and waiting for a route
 *   - `running`  — Agent.prompt() in flight
 *   - `completed`— shot executor finished cleanly
 *   - `cancelled`— explicit cancel
 *   - `failed`   — agent.errorMessage was set or runner.start threw
 *
 * `bigint` is used for unix-ms timestamps to avoid 32-bit overflow.
 */
import { bigint, index, integer, pgTable, text } from "drizzle-orm/pg-core";

export const workers = pgTable(
	"workers",
	{
		id: text("id").primaryKey(),
		kind: text("kind").notNull(),
		lifetime: text("lifetime").notNull(),
		parentSessionKey: text("parent_session_key").notNull(),
		sourceChannel: text("source_channel").notNull(),
		sourceChatId: text("source_chat_id").notNull(),
		sourceReplyTo: text("source_reply_to"),
		name: text("name").notNull(),
		initialPrompt: text("initial_prompt"),
		systemPrompt: text("system_prompt").notNull(),
		modelProvider: text("model_provider").notNull(),
		modelId: text("model_id").notNull(),
		status: text("status").notNull(),
		messagesJson: text("messages_json").notNull().default("[]"),
		containerName: text("container_name").notNull(),
		createdAt: bigint("created_at", { mode: "number" }).notNull(),
		startedAt: bigint("started_at", { mode: "number" }),
		updatedAt: bigint("updated_at", { mode: "number" }).notNull(),
		completedAt: bigint("completed_at", { mode: "number" }),
		error: text("error"),
		turnCount: integer("turn_count").notNull().default(0),
		sendPolicy: text("send_policy").notNull().default("explicit"),
	},
	(t) => ({
		statusIdx: index("workers_status_idx").on(t.status),
		parentIdx: index("workers_parent_idx").on(t.parentSessionKey),
	}),
);

export type WorkerRow = typeof workers.$inferSelect;
export type WorkerInsert = typeof workers.$inferInsert;
