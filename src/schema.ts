/**
 * Drizzle schema barrel — the single source the shared Postgres
 * connection (`src/db.ts`) and drizzle-kit (`drizzle.config.ts`) bind to.
 *
 *   - `workers`       — sub-agent persistence (see ./workers/schema.ts)
 *   - `settings`      — KV store (auth/model/channel config)
 *   - `tapes`         — tape.systems context model (see ./tape/schema.ts)
 *   - `tape_entries`  — append-only facts
 *   - `tape_files`    — external binary file metadata
 */
import { pgTable, text } from "drizzle-orm/pg-core";

export * from "./workers/schema.ts";
export * from "./tape/schema.ts";

export const settings = pgTable("settings", {
	key: text("key").primaryKey(),
	value: text("value").notNull(),
});

export type SettingRow = typeof settings.$inferSelect;
