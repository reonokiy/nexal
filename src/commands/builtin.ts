/**
 * Built-in slash commands registered at startup.
 */
import type { CommandRegistry } from "./registry.ts";
import type { GatewayClient } from "../gateway/client.ts";
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

const KNOWN_PROVIDERS = ["openrouter", "kimi-coding", "deepseek", "opencode-go"] as const;

interface ProvidersPayload {
	active: { provider: string; modelId: string } | null;
	providers: {
		name: string;
		hasKey: boolean;
	}[];
}

export function registerBuiltins(registry: CommandRegistry, gateway?: GatewayClient): void {
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
					const result = await gateway.invoke("gateway/list_agents", {});
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
				lines.push("", "/settings provider <name> url <url>", "/settings auth <provider> <key>", "/settings toolkey <name> <key>");
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
