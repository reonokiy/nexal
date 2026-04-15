/**
 * Session-scoped message debouncer.
 *
 * Direct TS port of `crates/channel-core/src/debounce.rs`. Three timing states:
 *
 *   1. Mentioned                → wait `debounceMs`, then dispatch.
 *   2. Unmentioned, within window of last mention → wait `delayMs`.
 *   3. Unmentioned, outside window                → tiny delay, let the
 *      model decide whether to reply.
 *
 * When the timer fires, all pending messages are merged into one
 * `IncomingMessage` (texts joined with `\n`, last message's metadata
 * wins, images concatenated) and handed to the handler.
 *
 * Implementation: single actor loop driven by an AsyncQueue. No shared
 * mutable state, no locks — mirrors the Rust mpsc+select! version.
 */
import type { IncomingMessage } from "./types.ts";

const UNMENTIONED_DELAY_MS = 100;

export interface DebounceConfig {
	debounceMs: number;
	delayMs: number;
	activeWindowMs: number;
}

export const DEFAULT_DEBOUNCE: DebounceConfig = {
	debounceMs: 1_000,
	delayMs: 10_000,
	activeWindowMs: 60_000,
};

export type MessageHandler = (msg: IncomingMessage) => Promise<void>;

interface Slot {
	resolve: (msg: IncomingMessage | null) => void;
}

/**
 * Minimal unbounded async queue. `push` is O(1); `take` awaits a value
 * and returns `null` once the queue is closed and drained.
 */
class AsyncQueue<T> {
	private readonly items: T[] = [];
	private readonly waiters: Array<(v: T | null) => void> = [];
	private closed = false;

	push(v: T): void {
		if (this.closed) return;
		const w = this.waiters.shift();
		if (w) w(v);
		else this.items.push(v);
	}

	take(): Promise<T | null> {
		const next = this.items.shift();
		if (next !== undefined) return Promise.resolve(next);
		if (this.closed) return Promise.resolve(null);
		return new Promise((resolve) => this.waiters.push(resolve));
	}

	close(): void {
		this.closed = true;
		for (const w of this.waiters.splice(0)) w(null);
	}
}

export class SessionRunner {
	private readonly queue = new AsyncQueue<IncomingMessage>();
	private readonly loop: Promise<void>;

	constructor(
		private readonly sessionId: string,
		private readonly config: DebounceConfig,
		private readonly handler: MessageHandler,
	) {
		this.loop = this.run();
	}

	process(msg: IncomingMessage): void {
		this.queue.push(msg);
	}

	async shutdown(): Promise<void> {
		this.queue.close();
		await this.loop;
	}

	private async run(): Promise<void> {
		let pending: IncomingMessage[] = [];
		let lastMentionedAt: number | null = null;
		let deadline: number | null = null;

		// Pump: repeatedly either take the next message OR wait for the
		// deadline, whichever fires first. `deadline === null` means we
		// have nothing pending, so we simply block on `take`.
		while (true) {
			if (deadline === null) {
				const msg = await this.queue.take();
				if (msg === null) return;
				const wait = this.nextDelayMs(msg, () => {
					lastMentionedAt = Date.now();
				}, lastMentionedAt);
				deadline = Date.now() + wait;
				pending.push(msg);
				continue;
			}

			const now = Date.now();
			const remaining = Math.max(0, deadline - now);
			const timer = new Promise<"timeout">((resolve) =>
				setTimeout(() => resolve("timeout"), remaining),
			);
			const race = await Promise.race([
				this.queue.take().then((m) => ({ kind: "msg", msg: m }) as const),
				timer.then(() => ({ kind: "timeout" }) as const),
			]);

			if (race.kind === "msg") {
				if (race.msg === null) {
					// Closed. Flush anything pending, then exit.
					if (pending.length > 0) await this.dispatch(pending);
					return;
				}
				const wait = this.nextDelayMs(race.msg, () => {
					lastMentionedAt = Date.now();
				}, lastMentionedAt);
				deadline = Date.now() + wait;
				pending.push(race.msg);
				continue;
			}

			// Timeout fired → dispatch the batch.
			const batch = pending;
			pending = [];
			deadline = null;
			await this.dispatch(batch);
		}
	}

	private nextDelayMs(
		msg: IncomingMessage,
		markMentioned: () => void,
		lastMentionedAt: number | null,
	): number {
		if (msg.isMentioned) {
			markMentioned();
			return this.config.debounceMs;
		}
		if (lastMentionedAt !== null && Date.now() - lastMentionedAt < this.config.activeWindowMs) {
			return this.config.delayMs;
		}
		return UNMENTIONED_DELAY_MS;
	}

	private async dispatch(batch: IncomingMessage[]): Promise<void> {
		const merged = mergeMessages(batch);
		if (!merged) return;
		try {
			await this.handler(merged);
		} catch (err) {
			console.error(`[debounce:${this.sessionId}] handler threw`, err);
		}
	}
}

export function mergeMessages(messages: IncomingMessage[]): IncomingMessage | null {
	if (messages.length === 0) return null;
	if (messages.length === 1) return messages[0]!;

	const base = messages[messages.length - 1]!;
	const combinedText = messages.map((m) => m.text).join("\n");
	const allImages = messages.flatMap((m) => m.images);

	return {
		...base,
		text: combinedText,
		isMentioned: true,
		images: allImages,
	};
}
