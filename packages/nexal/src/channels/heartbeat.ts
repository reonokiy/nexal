/**
 * Heartbeat channel — TS port of `crates/channel-heartbeat`.
 *
 * Fires a synthetic "system" IncomingMessage into session
 * `heartbeat:main` every N minutes, so the agent has a chance to
 * proactively review pending tasks and follow-ups.
 */
import type { Channel, IncomingMessage, OutgoingReply } from "./types.ts";

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

export class HeartbeatChannel implements Channel {
	readonly name = "heartbeat";
	private timer: ReturnType<typeof setInterval> | null = null;
	private stopped = false;

	constructor(private readonly config: HeartbeatChannelConfig = {}) {}

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		const minutes = this.config.intervalMinutes ?? 30;
		const text = this.config.text ?? DEFAULT_TEXT;
		console.log(`[heartbeat] firing every ${minutes} minute(s)`);

		const fire = () => {
			if (this.stopped) return;
			onMessage({
				channel: "heartbeat",
				chatId: "main",
				sender: "system",
				text,
				timestamp: Date.now(),
				isMentioned: true,
				metadata: {},
				images: [],
			});
		};

		this.timer = setInterval(fire, minutes * 60 * 1_000);
		// Skip the first immediate tick — let the system settle on startup
		// (matches the Rust behavior: `ticker.tick().await` before the loop).
		await new Promise<void>((resolve) => {
			const check = setInterval(() => {
				if (this.stopped) {
					clearInterval(check);
					resolve();
				}
			}, 1_000);
		});
	}

	async send(_reply: OutgoingReply): Promise<void> {
		// Heartbeat is pure input — replies bubble back through the
		// channel that originated the user-visible conversation. There's
		// nothing to send here.
	}

	async stop(): Promise<void> {
		this.stopped = true;
		if (this.timer !== null) clearInterval(this.timer);
		this.timer = null;
	}
}
