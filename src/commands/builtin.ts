/**
 * Built-in slash commands registered at startup.
 */
import type { CommandRegistry } from "./registry.ts";
import {
	deleteAuth,
	loadAuth,
	loadModelConfig,
	saveAuth,
	saveModelConfig,
} from "../settings.ts";
import { apiKeyEnvKey } from "../index.ts";

const KNOWN_PROVIDERS = [
	"anthropic",
	"openai",
	"openrouter",
	"google",
	"mistral",
] as const;

interface ProvidersPayload {
	active: { provider: string; modelId: string } | null;
	providers: {
		name: string;
		hasKey: boolean;
		envKey: string | null;
	}[];
}

export function registerBuiltins(registry: CommandRegistry): void {
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
						envKey: apiKeyEnvKey(name),
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
}
