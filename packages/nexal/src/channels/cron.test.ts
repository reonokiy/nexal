import { afterEach, describe, expect, test } from "bun:test";

import { CronChannel, type CronJob } from "./cron.ts";
import type { IncomingMessage } from "./types.ts";

const running: CronChannel[] = [];

async function runFor(
	ms: number,
	tickSecs: number,
	seed: (ch: CronChannel) => void,
): Promise<IncomingMessage[]> {
	const received: IncomingMessage[] = [];
	const ch = new CronChannel({ tickIntervalSecs: tickSecs });
	running.push(ch);
	seed(ch);
	ch.start((m) => received.push(m));
	await new Promise((r) => setTimeout(r, ms));
	return received;
}

afterEach(async () => {
	for (const ch of running.splice(0)) await ch.stop();
});

describe("CronChannel", () => {
	test("channel name is 'cron'", () => {
		expect(new CronChannel().name).toBe("cron");
	});

	test("addJob / listJobs / removeJob round-trip", () => {
		const ch = new CronChannel();
		ch.addJob(sampleJob({ id: "a" }));
		ch.addJob(sampleJob({ id: "b" }));
		expect(ch.listJobs().map((j) => j.id).sort()).toEqual(["a", "b"]);
		expect(ch.removeJob("a")).toBe(true);
		expect(ch.removeJob("a")).toBe(false); // already gone
		expect(ch.listJobs().map((j) => j.id)).toEqual(["b"]);
	});

	test("addJob stores a defensive copy so callers can't mutate internals", () => {
		const ch = new CronChannel();
		const job = sampleJob({ id: "c", label: "orig" });
		ch.addJob(job);
		// Mutate the caller's object.
		job.label = "changed";
		expect(ch.listJobs()[0]!.label).toBe("orig");
	});

	test("`every:N` schedule fires on the first tick past the interval", async () => {
		const received = await runFor(300, 0.05, (ch) => {
			ch.addJob(
				sampleJob({
					id: "j",
					schedule: "every:0", // fire whenever (0 → `secs <= 0` → disabled)
				}),
			);
		});
		// "every:0" is rejected (<=0) so nothing fires.
		expect(received).toEqual([]);
	});

	test("`every:0.05` (every 50ms) fires multiple times in 250ms", async () => {
		const received = await runFor(250, 0.05 /* 50ms tick */, (ch) => {
			ch.addJob(
				sampleJob({
					id: "j",
					schedule: "every:0.04", // 40ms interval
					label: "ping",
					message: "heartbeat",
					targetChannel: "test",
					targetChatId: "t1",
				}),
			);
		});
		expect(received.length).toBeGreaterThanOrEqual(2);
		expect(received[0]!.channel).toBe("test");
		expect(received[0]!.chatId).toBe("t1");
		expect(received[0]!.text).toContain("[cron:ping] heartbeat");
		expect(received[0]!.metadata.cron_job_id).toBe("j");
		expect(received[0]!.metadata.cron_label).toBe("ping");
	});

	test("disabled jobs never fire", async () => {
		const received = await runFor(200, 0.05, (ch) => {
			ch.addJob(sampleJob({ id: "j", schedule: "every:0.04", enabled: false }));
		});
		expect(received).toEqual([]);
	});

	test("`once:<ISO>` fires exactly once and self-deletes", async () => {
		const soon = new Date(Date.now() + 50).toISOString();
		const ch = new CronChannel({ tickIntervalSecs: 0.05 });
		running.push(ch);
		ch.addJob(
			sampleJob({
				id: "one",
				schedule: `once:${soon}`,
				message: "wake up",
			}),
		);
		const received: IncomingMessage[] = [];
		ch.start((m) => received.push(m));
		await new Promise((r) => setTimeout(r, 250));
		expect(received).toHaveLength(1);
		expect(received[0]!.text).toContain("wake up");
		expect(ch.listJobs()).toEqual([]);
	});

	test("`once:<ISO>` in the past fires immediately on the first tick", async () => {
		const past = new Date(Date.now() - 10_000).toISOString();
		const received = await runFor(150, 0.05, (ch) => {
			ch.addJob(sampleJob({ id: "back", schedule: `once:${past}` }));
		});
		expect(received).toHaveLength(1);
	});

	test("message body carries context when set", async () => {
		const received = await runFor(200, 0.05, (ch) => {
			ch.addJob(
				sampleJob({
					id: "j",
					schedule: "every:0.04",
					label: "note",
					message: "check logs",
					context: "from yesterday",
				}),
			);
		});
		expect(received.length).toBeGreaterThanOrEqual(1);
		expect(received[0]!.text).toContain("[cron:note] check logs");
		expect(received[0]!.text).toContain("Context from when this was scheduled:\nfrom yesterday");
	});

	test("crontab-style expression silently disables the job (parser TBD)", async () => {
		const origWarn = console.warn;
		(console as any).warn = () => undefined;
		try {
			const received = await runFor(200, 0.05, (ch) => {
				ch.addJob(sampleJob({ id: "c", schedule: "*/5 * * * *" }));
			});
			expect(received).toEqual([]);
		} finally {
			(console as any).warn = origWarn;
		}
	});

	test("send() is a no-op for the input-only cron channel", async () => {
		const ch = new CronChannel();
		await ch.send({ chatId: "whatever", text: "ignored" });
		expect(ch.listJobs()).toEqual([]);
	});
});

function sampleJob(over: Partial<CronJob> = {}): CronJob {
	return {
		id: "job-id",
		label: "sample",
		schedule: "every:60",
		targetChannel: "telegram",
		targetChatId: "-1",
		message: "ping",
		enabled: true,
		...over,
	};
}
