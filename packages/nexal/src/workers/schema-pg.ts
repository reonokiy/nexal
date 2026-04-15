/**
 * Postgres schema for the `workers` table (Drizzle).
 *
 * Mirror of `schema-sqlite.ts` — see that file for column semantics.
 * Uses `bigint` for unix-ms timestamps to avoid 32-bit overflow.
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
  created_at BIGINT NOT NULL,
  started_at BIGINT,
  updated_at BIGINT NOT NULL,
  completed_at BIGINT,
  error TEXT,
  turn_count INTEGER NOT NULL DEFAULT 0,
  send_policy TEXT NOT NULL DEFAULT 'explicit'
);
CREATE INDEX IF NOT EXISTS workers_status_idx ON workers(status);
CREATE INDEX IF NOT EXISTS workers_parent_idx ON workers(parent_session_key);
`;
