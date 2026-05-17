/**
 * Drizzle schema barrel — the single source the shared Postgres
 * connection (`src/db.ts`) and drizzle-kit (`drizzle.config.ts`) bind to.
 *
 *   - `workers`  — sub-agent persistence (see ./workers/schema.ts)
 *   - `settings` — KV store (auth/model/channel config); moved here from
 *                  the removed PGlite store.
 */
import { pgTable, text } from "drizzle-orm/pg-core";

export * from "./workers/schema.ts";

export const settings = pgTable("settings", {
	key: text("key").primaryKey(),
	value: text("value").notNull(),
});

export type SettingRow = typeof settings.$inferSelect;
