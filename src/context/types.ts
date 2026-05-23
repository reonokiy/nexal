import type { AgentMessage } from "@mariozechner/pi-agent-core";

/** Options for loading conversation context. */
export interface ContextLoadOptions {
	/** Maximum messages to keep (older ones truncated). Default 200. */
	maxMessages?: number;
}

/** A context store abstracts over tape + legacy DB persistence. */
export interface ContextStore {
	/** Load full conversation history for a session. */
	load(sessionKey: string, opts?: ContextLoadOptions): Promise<AgentMessage[]>;

	/** Save full conversation history (overwrites). */
	save(sessionKey: string, messages: AgentMessage[]): Promise<void>;

	/** Append only new messages since `fromIndex`. */
	appendDelta(sessionKey: string, fromIndex: number, messages: AgentMessage[]): Promise<void>;
}
