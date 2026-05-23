/**
 * Heartbeat channel — TS port of `crates/channel-heartbeat`.
 *
 * Fires a synthetic "system" IncomingMessage into session
 * `heartbeat:main` every N minutes, so the agent has a chance to
 * proactively review pending tasks and follow-ups.
 */
import type { IncomingMessage } from "./types.ts";
import { createIncomingMessage } from "./types.ts";
import { PollingChannel } from "./polling-base.ts";
import { createLog } from "../log.ts";
import { registerChannel } from "./factory.ts";

const log = createLog("heartbeat");

export interface HeartbeatChannelConfig {
	/** Default 30 minutes. */
	intervalMinutes?: number;
	/** Override the synthetic prompt text. */
	text?: string;
}

const DEFAULT_TEXT =
	"[heartbeat] This is a periodic check-in. Review pending tasks, " +
	"conversations, and proactively handle anything that needs attention. " +
	"If there is nothing to do, call no_response.";

export class HeartbeatChannel extends PollingChannel {
	readonly name = "heartbeat";
	protected readonly tickIntervalMs: number;
	private readonly text: string;

	constructor(private readonly config: HeartbeatChannelConfig = {}) {
		super();
		this.tickIntervalMs = (config.intervalMinutes ?? 30) * 60 * 1_000;
		this.text = config.text ?? DEFAULT_TEXT;
	}

	protected onStart(): void {
		log.info(`firing every ${this.tickIntervalMs / 60 / 1_000} minute(s)`);
	}

	protected onTick(): void {
		this.onMessage?.(
			createIncomingMessage({
				channel: "heartbeat",
				chatId: "main",
				sender: "system",
				text: this.text,
			}),
		);
	}
}

registerChannel("heartbeat", ({ cfg }) => {
	if (cfg.enabled !== true) return null;
	return new HeartbeatChannel({
		intervalMinutes:
			(cfg.intervalMins as number | undefined) ??
			(cfg.intervalMinutes as number | undefined),
	});
});
