/**
 * Built-in slash commands registered at startup.
 */
import type { CommandRegistry } from "./registry.ts";
import type { CommandContext } from "./registry.ts";
import type { GatewayClient } from "../gateway/index.ts";
import type { TapeEntry, TapeHandle, TapeInfo, TapeStore } from "../tape/index.ts";
import {
	deleteAuth,
	loadAuth,
	loadModelConfig,
	saveAuth,
	saveModelConfig,
	saveProviderConfig,
	loadProviderConfig,
	loadAllProviderConfigs,
	saveToolApiKey,
	loadAllToolApiKeys,
} from "../settings.ts";

const KNOWN_PROVIDERS = [
	"openrouter",
	"openai",
	"anthropic",
	"google",
	"deepseek",
	"kimi-coding",
	"opencode-go",
] as const;

interface ProvidersPayload {
	active: { provider: string; modelId: string } | null;
	providers: {
		name: string;
		hasKey: boolean;
	}[];
}

export interface TapesPayload {
	tapes: TapeInfo[];
}

export interface TapePayload {
	tape: TapeInfo;
}

export interface TapeEntriesPayload {
	tape: TapeInfo;
	entries: TapeEntry[];
	offset: number;
	limit: number;
	total: number;
	hasMore: boolean;
}

type GetTapeRef = (sessionKey: string) => Promise<TapeHandle | null>;

const MAX_TAPE_PAGE_SIZE = 500;

type ConfigureParseResult =
	| {
			ok: true;
			provider: string;
			modelId: string;
			apiKey: string;
			baseUrl?: string;
	  }
	| { ok: false; result: { text: string; error?: string } };

export function parseConfigureArgs(args: string[]): ConfigureParseResult {
	const usage =
		"Usage: /configure <provider> <model_id> <api_key> [--base-url <url>]\n" +
		"       /configure <provider> <model_id> --base-url <url>";
	const [provider, modelId, ...rest] = args;
	if (!provider || !modelId) {
		return { ok: false, result: { text: usage, error: "missing provider or model_id" } };
	}

	let baseUrl: string | undefined;
	const keyParts: string[] = [];
	for (let i = 0; i < rest.length; i++) {
		const part = rest[i]!;
		if (part === "--base-url" || part === "--url") {
			const value = rest[++i];
			if (!value) {
				return { ok: false, result: { text: usage, error: `${part} requires a URL` } };
			}
			baseUrl = value;
			continue;
		}
		keyParts.push(part);
	}

	const apiKey = keyParts.join(" ").trim();
	if (!apiKey && !baseUrl) {
		return { ok: false, result: { text: usage, error: "missing api_key or --base-url" } };
	}
	return { ok: true, provider, modelId, apiKey, ...(baseUrl ? { baseUrl } : {}) };
}

export function registerBuiltins(
	registry: CommandRegistry,
	gateway?: GatewayClient,
	tapeStore?: TapeStore,
	getTapeRef?: GetTapeRef,
): void {
	registry.register({
		name: "help",
		description: "Show available commands",
		async execute(_ctx, _args) {
			const lines = registry
				.list()
				.map((c) => `/${c.name} — ${c.description}`);
			return { text: lines.join("\n") };
		},
	});

	registry.register({
		name: "model",
		description:
			"View or set the model (e.g. /model anthropic claude-sonnet-4-6)",
		async execute(_ctx, args) {
			if (args.length < 2) {
				const saved = await loadModelConfig();
				if (saved) {
					return {
						text: `Current model: ${saved.provider} / ${saved.modelId}`,
						data: { provider: saved.provider, modelId: saved.modelId },
					};
				}
				return {
					text: "No model configured. Usage: /model <provider> <model_id>",
					data: null,
				};
			}
			const [provider, modelId] = args;
			await saveModelConfig(provider!, modelId!);
			return {
				text: `Model set to ${provider} / ${modelId}. Restart nexal to apply.`,
				data: { provider, modelId },
			};
		},
	});

	registry.register({
		name: "apikey",
		description: "Save an API key (e.g. /apikey anthropic sk-...)",
		async execute(_ctx, args) {
			const [provider, ...rest] = args;
			if (!provider) {
				return {
					text:
						"Usage: /apikey <provider> <key>\n" +
						"       /apikey <provider> --clear",
				};
			}
			if (rest[0] === "--clear" || rest.length === 0) {
				await deleteAuth(provider);
				return { text: `Cleared API key for ${provider}.` };
			}
			const key = rest.join(" ").trim();
			await saveAuth({ provider, apiKey: key });
			return {
				text: `Saved API key for ${provider}. Restart nexal to apply.`,
			};
		},
	});

	registry.register({
		name: "configure",
		description: "Configure provider, model, API key, and optional base URL in one step",
		async execute(_ctx, args) {
			const parsed = parseConfigureArgs(args);
			if (!parsed.ok) return parsed.result;

			const { provider, modelId, apiKey, baseUrl } = parsed;
			if (baseUrl) {
				const existing = await loadProviderConfig(provider);
				await saveProviderConfig(provider, { ...existing, base_url: baseUrl });
			}
			if (apiKey) await saveAuth({ provider, apiKey });
			await saveModelConfig(provider, modelId);

			const lines = [
				`Configured ${provider} / ${modelId}.`,
				apiKey ? "API key saved." : "API key unchanged.",
			];
			if (baseUrl) lines.push(`Base URL set to ${baseUrl}.`);
			lines.push("Restart nexal to apply the runtime model.");
			return {
				text: lines.join("\n"),
				data: {
					provider,
					modelId,
					hasKey: Boolean(apiKey),
					...(baseUrl ? { baseUrl } : {}),
				},
			};
		},
	});

	registry.register({
		name: "providers",
		description: "List known providers and their auth status",
		async execute(_ctx, _args) {
			const active = await loadModelConfig();
			const providers = await Promise.all(
				KNOWN_PROVIDERS.map(async (name) => {
					const auth = await loadAuth(name);
					return {
						name,
						hasKey: !!auth?.apiKey,
					};
				}),
			);
			const payload: ProvidersPayload = { active, providers };

			const lines = providers.map(
				(p) =>
					`${p.name.padEnd(11)} ${p.hasKey ? "✓ key" : "  --"}` +
					(active?.provider === p.name ? "  (active)" : ""),
			);
			if (active) lines.push("", `active: ${active.provider} / ${active.modelId}`);
			return { text: lines.join("\n"), data: payload };
		},
	});

	registry.register({
		name: "status",
		description: "Show nexal system status",
		async execute(_ctx, _args) {
			const uptime = process.uptime();
			const mem = process.memoryUsage();
			const hours = Math.floor(uptime / 3600);
			const mins = Math.floor((uptime % 3600) / 60);
			const secs = Math.floor(uptime % 60);
			const rss = (mem.rss / 1024 / 1024).toFixed(1);
			return {
				text: [
					`uptime: ${hours}h ${mins}m ${secs}s`,
					`memory: ${rss} MB RSS`,
					`pid: ${process.pid}`,
				].join("\n"),
			};
		},
	});

	if (gateway) {
		registry.register({
			name: "sandboxes",
			description: "List running sandbox containers",
			async execute(_ctx, _args) {
				try {
					const result = await gateway.listAgents();
					const { agents } = result as { agents: Array<{ agent_id: string; container_name: string; created_at_unix_ms: number }> };
					if (agents.length === 0) {
						return { text: "No sandboxes running." };
					}
					const lines = agents.map((a) => {
						const age = Math.floor((Date.now() - a.created_at_unix_ms) / 1000);
						const ageStr = age < 60 ? `${age}s` : age < 3600 ? `${Math.floor(age / 60)}m` : `${Math.floor(age / 3600)}h`;
						return `${a.container_name.padEnd(30)} ${a.agent_id.slice(0, 12)}…  ${ageStr}`;
					});
					return { text: lines.join("\n"), data: result };
				} catch (err) {
					return { text: `Failed to list sandboxes: ${err}` };
				}
			},
		});
	}

	if (tapeStore && getTapeRef) {
		const resolveTapeRef = getTapeRef;
		async function requireAllowedTape(
			ctx: CommandContext,
			tapeId: string | undefined,
		): Promise<
			| { ok: true; handle: TapeHandle }
			| { ok: false; result: { text: string; error: string } }
		> {
			if (!tapeId) {
				return {
					ok: false,
					result: { text: "Usage: /tape <tape_id>", error: "missing tape_id" },
				};
			}
			const allowed = await resolveTapeRef(`${ctx.channel}:${ctx.chatId}`);
			if (!allowed || allowed.tapeId !== tapeId) {
				return {
					ok: false,
					result: { text: `Tape not found: ${tapeId}`, error: "tape not found" },
				};
			}
			return { ok: true, handle: { tapeId } };
		}

		registry.register({
			name: "tapes",
			description: "List tapes for the current session",
			async execute(ctx, _args) {
				const ref = await getTapeRef(`${ctx.channel}:${ctx.chatId}`);
				if (!ref) {
					return {
						text: "No tapes found.",
						data: { tapes: [] } satisfies TapesPayload,
					};
				}
				const tape = await tapeStore.info(ref);
				const tapes = tape.entries > 0 ? [tape] : [];
				const lines = tapes.map((item) => {
					const tape = item;
					const shortId = tape.id.length > 12 ? `${tape.id.slice(0, 12)}…` : tape.id;
					const anchor = tape.lastAnchor ? ` last anchor: ${tape.lastAnchor}` : " no anchor";
					return `${shortId}  ${tape.entries} entries, ${tape.anchors} anchors,${anchor}`;
				});
				return {
					text: lines.length ? lines.join("\n") : "No tapes found.",
					data: { tapes } satisfies TapesPayload,
				};
			},
		});

		registry.register({
			name: "tape",
			description: "Read current session tape metadata (usage: /tape <tape_id>)",
			async execute(ctx, args) {
				const [tapeId] = args;
				const allowed = await requireAllowedTape(ctx, tapeId);
				if (!allowed.ok) return allowed.result;
				const tape = await tapeStore.info(allowed.handle);
				return {
					text: `Tape ${tape.id}: ${tape.entries} entries.`,
					data: { tape } satisfies TapePayload,
				};
			},
		});

		registry.register({
			name: "tape_entries",
			description: "Read current session tape entries by page (usage: /tape_entries <tape_id> [offset] [limit])",
			async execute(ctx, args) {
				const [tapeId, offsetArg, limitArg] = args;
				const allowed = await requireAllowedTape(ctx, tapeId);
				if (!allowed.ok) return allowed.result;
				const offset = Math.max(0, Number.parseInt(offsetArg ?? "0", 10) || 0);
				const requestedLimit = Number.parseInt(limitArg ?? "100", 10) || 100;
				const limit = Math.min(MAX_TAPE_PAGE_SIZE, Math.max(1, requestedLimit));
				const [tape, entries] = await Promise.all([
					tapeStore.info(allowed.handle),
					tapeStore.readPage(allowed.handle, { offset, limit }),
				]);
				return {
					text: `Tape ${tape.id}: entries ${offset + 1}-${offset + entries.length} of ${tape.entries}.`,
					data: {
						tape,
						entries,
						offset,
						limit,
						total: tape.entries,
						hasMore: offset + entries.length < tape.entries,
					} satisfies TapeEntriesPayload,
				};
			},
		});
	}

	// ── Settings management commands ─────────────────────────────────

	registry.register({
		name: "settings",
		description: "Manage provider config, auth, and tool keys (stored in DB)",
		async execute(_ctx, args) {
			const [sub, ...rest] = args;
			if (!sub) {
				// Show summary of all settings
				const model = await loadModelConfig();
				const providers = await loadAllProviderConfigs();
				const toolKeys = await loadAllToolApiKeys();
				const data = {
					model: model ?? null,
					providers: Object.entries(providers).map(([name, cfg]) => ({ name, ...cfg })),
					toolKeys: Object.keys(toolKeys),
				};
				const lines: string[] = [];
				if (model) lines.push(`model: ${model.provider} / ${model.modelId}`);
				else lines.push("model: not set");
				for (const [name, cfg] of Object.entries(providers)) {
					lines.push(`provider ${name}: ${String(cfg.base_url ?? "(default)")}`);
				}
				for (const name of Object.keys(toolKeys)) {
					lines.push(`tool key: ${name} ✓`);
				}
				lines.push(
					"",
					"/configure <provider> <model_id> <api_key> [--base-url <url>]",
					"/settings provider <name> url <url>",
					"/settings auth <provider> <key>",
					"/settings toolkey <name> <key>",
				);
				return { text: lines.join("\n"), data };
			}

			switch (sub) {
				case "provider": {
					const [name, prop, ...vals] = rest;
					if (!name) {
						return { text: "Usage: /settings provider <name> [url <url>]", error: "missing name" };
					}
					if (!prop) {
						const cfg = await loadProviderConfig(name);
						return {
							text: cfg ? `provider ${name}: ${JSON.stringify(cfg, null, 2)}` : `provider ${name}: not configured`,
							data: cfg ?? null,
						};
					}
					if (prop === "url" && vals.length > 0) {
						const url = vals.join(" ");
						const existing = await loadProviderConfig(name);
						await saveProviderConfig(name, { ...existing, base_url: url });
						return { text: `provider ${name} base_url set to ${url}` };
					}
					return { text: `Unknown provider option: ${prop}. Use "url"` };
				}
				case "auth": {
					const [provider, ...keyParts] = rest;
					if (!provider) {
						return { text: "Usage: /settings auth <provider> <apiKey>" };
					}
					if (keyParts.length === 0) {
						const auth = await loadAuth(provider);
						return { text: auth ? `auth ${provider}: key saved ✓` : `auth ${provider}: not set` };
					}
					const key = keyParts.join(" ");
					await saveAuth({ provider, apiKey: key });
					return { text: `auth ${provider} saved.` };
				}
				case "toolkey": {
					const [name, ...keyParts] = rest;
					if (!name) {
						return { text: "Usage: /settings toolkey <name> <apiKey>" };
					}
					if (keyParts.length === 0) {
						const keys = await loadAllToolApiKeys();
						return { text: keys[name] ? `tool key ${name}: saved ✓` : `tool key ${name}: not set` };
					}
					const key = keyParts.join(" ");
					await saveToolApiKey(name, key);
					return { text: `tool key ${name} saved.` };
				}
				default:
					return { text: `Unknown sub-command: ${sub}. Use provider, auth, or toolkey.` };
			}
		},
	});
}
