/**
 * SQLite schema for the `workers` table (Drizzle).
 *
 * Mirror of `schema-pg.ts` — column shape must stay identical so the
 * driver-agnostic `WorkerStore` queries work either way.
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
 */
import { index, integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const workers = sqliteTable(
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
		createdAt: integer("created_at").notNull(),
		startedAt: integer("started_at"),
		updatedAt: integer("updated_at").notNull(),
		completedAt: integer("completed_at"),
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

export const CREATE_SQL = `
CREATE TABLE IF NOT EXISTS workers (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('coordinator','executor')),
  lifetime TEXT NOT NULL CHECK (lifetime IN ('persistent','shot')),
  parent_session_key TEXT NOT NULL,
  source_channel TEXT NOT NULL,
  source_chat_id TEXT NOT NULL,
  source_reply_to TEXT,
  name TEXT NOT NULL,
  initial_prompt TEXT,
  system_prompt TEXT NOT NULL,
  model_provider TEXT NOT NULL,
  model_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('spawning','idle','running','completed','cancelled','failed')),
  messages_json TEXT NOT NULL DEFAULT '[]',
  container_name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  updated_at INTEGER NOT NULL,
  completed_at INTEGER,
  error TEXT,
  turn_count INTEGER NOT NULL DEFAULT 0,
  send_policy TEXT NOT NULL DEFAULT 'explicit'
);
CREATE INDEX IF NOT EXISTS workers_status_idx ON workers(status);
CREATE INDEX IF NOT EXISTS workers_parent_idx ON workers(parent_session_key);
`;
