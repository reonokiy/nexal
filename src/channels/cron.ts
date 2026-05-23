/**
 * Cron channel — TS port of `crates/channel-cron`.
 *
 * Three job types (same wire format as the Rust implementation):
 *   - `every:N`  — fire every N seconds
 *   - `once:<ISO8601>` — fire once at the target time, then self-delete
 *   - "`M H D M W`" — standard 5-field crontab expression
 *
 * Unlike the Rust version, this initial port stores jobs **in memory**;
 * persistence across restarts will land with the SQLite state port.
 * The `addJob` / `removeJob` / `listJobs` API is exposed so a cron
 * management tool (registered as an AgentTool) can drive it.
 *
 * When a job fires, it dispatches an IncomingMessage to the stored
 * `targetChannel:targetChatId` — the AgentPool routes it just like any
 * other channel input.
 */

import type { IncomingMessage } from "./types.ts";
import { createIncomingMessage } from "./types.ts";
import { PollingChannel } from "./polling-base.ts";
import { createLog } from "../log.ts";
import { registerChannel } from "./factory.ts";

const log = createLog("cron");

export interface CronJob {
	id: string;
	label: string;
	schedule: string; // "every:N" | "once:<ISO>" | "<cron expr>"
	targetChannel: string;
	targetChatId: string;
	message: string;
	context?: string;
	enabled: boolean;
	lastRunAt?: number;
}

export interface CronChannelConfig {
	tickIntervalSecs?: number;
}

export class CronChannel extends PollingChannel {
	readonly name = "cron";
	protected readonly tickIntervalMs: number;
	private readonly jobs = new Map<string, CronJob>();

	constructor(private readonly config: CronChannelConfig = {}) {
		super();
		this.tickIntervalMs = (config.tickIntervalSecs ?? 15) * 1_000;
	}

	addJob(job: CronJob): void {
		this.jobs.set(job.id, { ...job });
	}

	removeJob(id: string): boolean {
		return this.jobs.delete(id);
	}

	listJobs(): CronJob[] {
		return [...this.jobs.values()];
	}

	protected onStart(): void {
		log.info(`checking job schedules every ${this.tickIntervalMs / 1_000}s`);
	}

	protected onTick(): void {
		const now = Date.now();
		for (const job of this.jobs.values()) {
			if (!job.enabled) continue;
			if (!shouldFire(job, now)) continue;

			let text = `[cron:${job.label}] ${job.message}`;
			if (job.context) text += `\n\nContext from when this was scheduled:\n${job.context}`;

			this.onMessage?.(
				createIncomingMessage({
					channel: job.targetChannel,
					chatId: job.targetChatId,
					sender: "cron",
					text,
					timestamp: now,
					metadata: { cron_job_id: job.id, cron_label: job.label },
				}),
			);

			job.lastRunAt = now;

			// One-shot jobs self-delete after firing.
			if (job.schedule.startsWith("once:")) this.jobs.delete(job.id);
		}
	}
}

function shouldFire(job: CronJob, nowMs: number): boolean {
	if (job.schedule.startsWith("every:")) {
		const secs = Number(job.schedule.slice("every:".length));
		if (!Number.isFinite(secs) || secs <= 0) return false;
		if (job.lastRunAt === undefined) return true;
		return nowMs - job.lastRunAt >= secs * 1_000;
	}
	if (job.schedule.startsWith("once:")) {
		if (job.lastRunAt !== undefined) return false;
		const target = Date.parse(job.schedule.slice("once:".length));
		if (!Number.isFinite(target)) return false;
		return nowMs >= target;
	}
	// Standard 5-field cron expression. A full parser is out of scope
	// for this initial port — return false (effectively disabled) and
	// warn once. Swap in a proper parser (e.g. `cron-parser`) before
	// relying on crontab-style jobs.
	warnOnce(job.schedule);
	return false;
}

const warned = new Set<string>();
function warnOnce(expr: string): void {
	if (warned.has(expr)) return;
	warned.add(expr);
	log.warn(`crontab expression "${expr}" is not supported yet, install a cron parser to enable`);
}

registerChannel("cron", ({ cfg }) => {
	if (cfg.enabled !== true) return null;
	return new CronChannel({
		tickIntervalSecs: cfg.tickIntervalSecs as number | undefined,
	});
});
