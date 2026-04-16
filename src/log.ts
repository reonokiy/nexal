/**
 * Centralized logging via consola.
 *
 * Usage:
 *   import { log } from "./log.ts";          // root logger, tag "nexal"
 *   import { createLog } from "./log.ts";
 *   const log = createLog("ws");             // tagged child logger
 */
import { createConsola } from "consola";

export const log = createConsola({ defaults: { tag: "nexal" } });

export function createLog(tag: string) {
	return log.withTag(tag);
}
