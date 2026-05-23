/**
 * GitHub channel — account-level issue/PR tracking via the Notifications
 * API, with bidirectional replies (agent answers become issue/PR
 * comments).
 *
 * Tracking source: `GET /notifications` covers every repo the
 * authenticated account participates in / watches (issues, PRs,
 * comments, @mentions, review requests, CI). GitHub keeps read state
 * server-side, so marking a thread read after dispatch *is* our cursor —
 * no local persistence needed, restart-safe (at-least-once delivery into
 * the AgentPool).
 *
 * chatId = "owner/repo#<number>" so each issue/PR is its own agent
 * session (thread context). `send()` derives the comment endpoint purely
 * from chatId (`POST /repos/{owner}/{repo}/issues/{n}/comments`, shared
 * by issue and PR conversation timelines) — stateless, restart-safe.
 *
 * Loop prevention: on startup `GET /user` resolves the bot's own login;
 * a thread whose latest comment author is the bot is marked read and
 * skipped instead of re-dispatched.
 *
 * Token: a classic PAT with scopes `notifications` (read threads) and
 * `repo` (private-repo content + posting comments). Fine-grained PATs
 * have unreliable Notifications API support — classic is recommended.
 * CI (`ci_activity` / CheckSuite) threads only appear in
 * `/notifications` if enabled in the account's notification settings.
 */

import type { Channel, IncomingMessage, OutgoingReply } from "./types.ts";
import { createLog } from "../log.ts";
import { registerChannel } from "./factory.ts";

const log = createLog("github");

const API = "https://api.github.com";
const BODY_LIMIT = 4_000;
const DEFAULT_POLL_SECS = 60;
const DEFAULT_SUBJECT_TYPES = ["Issue", "PullRequest", "CheckSuite"];

export interface GitHubChannelConfig {
	/** Classic PAT, scopes: notifications + repo. */
	token: string;
	/** Floor for poll interval (secs). Effective = max(this, X-Poll-Interval). */
	pollIntervalSecs?: number;
	/** Notification `reason` allowlist. Empty/undefined = all reasons. */
	reasons?: string[];
	/** `subject.type` allowlist. Default Issue/PullRequest/CheckSuite. */
	subjectTypes?: string[];
}

interface NotificationThread {
	id: string;
	unread: boolean;
	reason: string;
	updated_at: string;
	url: string;
	subject: {
		title: string;
		url: string | null;
		latest_comment_url: string | null;
		type: string;
	};
	repository: { full_name: string };
}

interface SubjectOrComment {
	number?: number;
	title?: string;
	body?: string | null;
	html_url?: string;
	state?: string;
	user?: { login: string };
}

export class GitHubChannel implements Channel {
	readonly name = "github";
	private stopped = false;
	private selfLogin = "";
	private lastModified: string | null = null;
	private wake: (() => void) | null = null;

	constructor(private readonly config: GitHubChannelConfig) {}

	async start(onMessage: (msg: IncomingMessage) => void): Promise<void> {
		try {
			const me = await this.json<{ login: string }>("GET", "/user");
			this.selfLogin = me.login;
		} catch (err) {
			log.error("GET /user failed — token invalid? GitHub channel inert", err);
			return;
		}
		log.info(`authenticated as @${this.selfLogin}, polling notifications`);

		const floor = (this.config.pollIntervalSecs ?? DEFAULT_POLL_SECS) * 1_000;
		while (!this.stopped) {
			let waitMs = floor;
			try {
				const serverSecs = await this.tick(onMessage);
				if (serverSecs > 0) waitMs = Math.max(floor, serverSecs * 1_000);
			} catch (err) {
				log.error("poll cycle failed, retrying after interval", err);
			}
			await this.sleep(waitMs);
		}
	}

	/** One poll cycle. Returns the server's X-Poll-Interval (secs, 0 if absent). */
	private async tick(onMessage: (msg: IncomingMessage) => void): Promise<number> {
		const headers: Record<string, string> = {};
		if (this.lastModified) headers["if-modified-since"] = this.lastModified;
		const resp = await this.request("GET", "/notifications", undefined, headers);

		const pollHeader = Number(resp.headers.get("x-poll-interval") ?? "0");
		const serverSecs = Number.isFinite(pollHeader) ? pollHeader : 0;

		if (resp.status === 304) return serverSecs; // nothing new, no rate cost
		if (!resp.ok) throw new Error(`GET /notifications: ${resp.status}`);

		const lm = resp.headers.get("last-modified");
		if (lm) this.lastModified = lm;

		const threads = (await resp.json()) as NotificationThread[];
		const subjectTypes = this.config.subjectTypes ?? DEFAULT_SUBJECT_TYPES;
		const reasons = this.config.reasons ?? [];

		for (const t of threads) {
			if (this.stopped) break;
			if (!t.unread) continue;
			if (!subjectTypes.includes(t.subject.type)) continue;
			if (reasons.length > 0 && !reasons.includes(t.reason)) continue;
			try {
				await this.handleThread(t, onMessage);
			} catch (err) {
				log.error(`thread ${t.id} (${t.repository.full_name}) failed`, err);
			}
		}
		return serverSecs;
	}

	private async handleThread(
		t: NotificationThread,
		onMessage: (msg: IncomingMessage) => void,
	): Promise<void> {
		const subjectUrl = t.subject.url;
		const subject = subjectUrl ? await this.jsonAbs<SubjectOrComment>(subjectUrl) : null;

		const commentUrl = t.subject.latest_comment_url;
		const comment =
			commentUrl && commentUrl !== subjectUrl
				? await this.jsonAbs<SubjectOrComment>(commentUrl)
				: null;

		const author = comment?.user?.login ?? subject?.user?.login ?? "unknown";
		// Loop guard: our own comment (or self-opened item w/ no comments).
		if (author === this.selfLogin) {
			await this.markRead(t.id);
			return;
		}

		const number = subject?.number;
		const repo = t.repository.full_name;
		const chatId = number !== undefined ? `${repo}#${number}` : `${repo}@${t.id}`;
		const htmlUrl = comment?.html_url ?? subject?.html_url ?? "";
		const title = subject?.title ?? t.subject.title;
		const state = subject?.state ? ` (${subject.state})` : "";
		const content = (comment?.body ?? subject?.body ?? "").slice(0, BODY_LIMIT);

		const text =
			`[github:${t.reason}] ${repo}#${number ?? "?"} "${title}"${state} by @${author}\n` +
			`${htmlUrl}\n\n${content}`;

		onMessage({
			channel: "github",
			chatId,
			sender: author,
			text,
			timestamp: Date.parse(t.updated_at) || Date.now(),
			isMentioned: true,
			metadata: {
				thread_id: t.id,
				repo,
				subject_type: t.subject.type,
				reason: t.reason,
				number: number ?? null,
				html_url: htmlUrl,
			},
			images: [],
		});
		log.info(`dispatched ${chatId} (${t.reason})`);

		await this.markRead(t.id);
	}

	async send(reply: OutgoingReply): Promise<void> {
		const m = /^(.+?)\/(.+?)#(\d+)$/.exec(reply.chatId);
		if (!m) {
			log.warn(`send() skipped, non-issue chatId "${reply.chatId}"`);
			return;
		}
		const [, owner, repo, number] = m;
		await this.json("POST", `/repos/${owner}/${repo}/issues/${number}/comments`, {
			body: reply.text,
		});
		log.info(`commented on ${reply.chatId}`);
	}

	async stop(): Promise<void> {
		this.stopped = true;
		this.wake?.();
	}

	private async markRead(threadId: string): Promise<void> {
		const resp = await this.request("PATCH", `/notifications/threads/${threadId}`);
		if (!resp.ok && resp.status !== 205) {
			log.warn(`mark-read ${threadId}: ${resp.status} (will redeliver next poll)`);
		}
	}

	// --- HTTP helpers (raw fetch, no SDK; mirrors telegram.ts) ----------

	private async request(
		method: string,
		path: string,
		body?: unknown,
		extraHeaders?: Record<string, string>,
	): Promise<Response> {
		return this.requestAbs(`${API}${path}`, method, body, extraHeaders);
	}

	private async requestAbs(
		url: string,
		method: string,
		body?: unknown,
		extraHeaders?: Record<string, string>,
	): Promise<Response> {
		return fetch(url, {
			method,
			headers: {
				authorization: `Bearer ${this.config.token}`,
				accept: "application/vnd.github+json",
				"x-github-api-version": "2022-11-28",
				...(body !== undefined ? { "content-type": "application/json" } : {}),
				...extraHeaders,
			},
			body: body !== undefined ? JSON.stringify(body) : undefined,
		});
	}

	private async json<T>(method: string, path: string, body?: unknown): Promise<T> {
		const resp = await this.request(method, path, body);
		if (!resp.ok) throw new Error(`${method} ${path}: ${resp.status}`);
		return (await resp.json()) as T;
	}

	private async jsonAbs<T>(url: string): Promise<T> {
		const resp = await this.requestAbs(url, "GET");
		if (!resp.ok) throw new Error(`GET ${url}: ${resp.status}`);
		return (await resp.json()) as T;
	}

	/** Interruptible sleep — `stop()` wakes it immediately. */
	private sleep(ms: number): Promise<void> {
		return new Promise<void>((resolve) => {
			const timer = setTimeout(() => {
				this.wake = null;
				resolve();
			}, ms);
			this.wake = () => {
				clearTimeout(timer);
				this.wake = null;
				resolve();
			};
		});
	}
}

registerChannel("github", ({ cfg }) => {
	const token = cfg.token as string | undefined;
	if (!token || cfg.enabled !== true) return null;
	return new GitHubChannel({
		token,
		pollIntervalSecs: cfg.pollIntervalSecs as number | undefined,
		reasons: cfg.reasons as string[] | undefined,
		subjectTypes: cfg.subjectTypes as string[] | undefined,
	});
});
