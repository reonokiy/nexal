/**
 * Built-in slash commands registered at startup.
 */
import type { CommandRegistry } from "./registry.ts";
import { loadModelConfig, saveModelConfig } from "../settings.ts";

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
		description: "View or set the model (e.g. /model anthropic claude-sonnet-4-6)",
		async execute(_ctx, args) {
			if (args.length < 2) {
				const saved = await loadModelConfig();
				if (saved) {
					return { text: `Current model: ${saved.provider} / ${saved.modelId}` };
				}
				return { text: "No model configured. Usage: /model <provider> <model_id>" };
			}
			const [provider, modelId] = args;
			await saveModelConfig(provider!, modelId!);
			return { text: `Model set to ${provider} / ${modelId}. Restart nexal to apply.` };
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
