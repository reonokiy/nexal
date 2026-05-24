/**
 * Agent notifications — async events pushed from agent → gateway → frontend.
 *
 * Delivered via `agent/notify`.
 */

import type { StreamKind } from "./methods.ts";

export interface ProcessOutputNotif {
	process_id: string;
	stream: StreamKind;
	seq: number;
	chunk: Uint8Array;
}
export interface ProcessExitedNotif {
	process_id: string;
	seq: number;
	exit_code: number | null;
}
export interface ProcessClosedNotif {
	process_id: string;
	seq: number;
}

/** Map of agent notification method name → params shape. */
export interface AgentNotifications {
	"process/output": ProcessOutputNotif;
	"process/exited": ProcessExitedNotif;
	"process/closed": ProcessClosedNotif;
}

/** Discriminated union — narrow by `notif.method`. */
export type AgentNotification = {
	[K in keyof AgentNotifications]: {
		agentId: string;
		method: K;
		params: AgentNotifications[K];
	};
}[keyof AgentNotifications];

/** Catch-all for notification methods we don't model yet. */
export interface UnknownAgentNotification {
	agentId: string;
	method: string;
	params: unknown;
}
