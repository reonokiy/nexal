import type { Channel, IncomingMessage, OutgoingReply } from "./types.ts";
import { waitUntilStopped } from "./types.ts";

/**
 * Base class for interval-based polling channels (cron, heartbeat).
 * Subclasses implement onTick() to emit messages and onStart() for setup.
 */
export abstract class PollingChannel implements Channel {
	abstract readonly name: string;
	protected stopped = false;
	private timer: ReturnType<typeof setInterval> | null = null;

	/** Interval between ticks in milliseconds. */
	protected abstract readonly tickIntervalMs: number;

	/** Called once when start() is invoked. Use for initialization. */
	protected abstract onStart(): void;

	/** Called on each interval tick. Push messages via the onMessage callback. */
	protected abstract onTick(): void;

	/** Override to provide the message callback. */
	protected onMessage: ((msg: IncomingMessage) => void) | null = null;

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		this.onMessage = onMessage;
		this.onStart();
		this.timer = setInterval(() => {
			if (!this.stopped) this.onTick();
		}, this.tickIntervalMs);
		await waitUntilStopped(() => this.stopped);
	}

	async send(_reply: OutgoingReply): Promise<void> {
		// Input-only channels don't support replies.
	}

	async stop(): Promise<void> {
		this.stopped = true;
		if (this.timer !== null) clearInterval(this.timer);
		this.timer = null;
	}
}
