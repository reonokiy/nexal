export interface ModelOption {
	id: string;
	label: string;
	icon?: string;
	config?: {
		reasoningEffort?: "low" | "medium" | "high";
		thinking?: boolean;
		endpointKind?: "openai-compatible" | "anthropic";
	};
}

export interface ProviderPreset {
	label: string;
	icon: string;
	summary: string;
	signupUrl: string;
	models: ModelOption[];
	apiKeyPlaceholder?: string;
	baseUrlPlaceholder?: string;
	baseUrlLabel?: string;
	showBaseUrl?: boolean;
	warn?: string;
}

export const PRESETS: Record<string, ProviderPreset> = {
	openrouter: {
		label: "OpenRouter",
		icon: "openrouter",
		summary: "Single key for Claude, GPT, DeepSeek, Kimi and other hosted models.",
		signupUrl: "https://openrouter.ai/keys",
		apiKeyPlaceholder: "sk-or-...",
		models: [
			{ id: "openai/gpt-4o", label: "GPT-4o", icon: "openai" },
			{ id: "anthropic/claude-sonnet-4-5", label: "Claude Sonnet 4.5", icon: "claude" },
			{ id: "deepseek/deepseek-chat", label: "DeepSeek Chat", icon: "deepseek" },
			{
				id: "moonshotai/kimi-k2-thinking",
				label: "Kimi K2 Thinking",
				icon: "kimi",
				config: { thinking: true, reasoningEffort: "medium" },
			},
		],
	},
	openai: {
		label: "OpenAI",
		icon: "openai",
		summary: "Direct OpenAI API access for GPT models.",
		signupUrl: "https://platform.openai.com/api-keys",
		apiKeyPlaceholder: "sk-...",
		models: [
			{ id: "gpt-5.4", label: "GPT-5.4", icon: "openai", config: { reasoningEffort: "medium" } },
			{ id: "gpt-4o", label: "GPT-4o", icon: "openai" },
		],
	},
	anthropic: {
		label: "Anthropic",
		icon: "anthropic",
		summary: "Direct Anthropic API access for Claude models.",
		signupUrl: "https://console.anthropic.com/settings/keys",
		apiKeyPlaceholder: "sk-ant-...",
		models: [
			{ id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6", icon: "claude" },
			{ id: "claude-opus-4-5", label: "Claude Opus 4.5", icon: "claude", config: { reasoningEffort: "high" } },
		],
	},
	google: {
		label: "Google Gemini",
		icon: "google",
		summary: "Direct Google Generative AI access for Gemini models.",
		signupUrl: "https://aistudio.google.com/apikey",
		apiKeyPlaceholder: "AIza...",
		models: [
			{ id: "gemini-2.5-flash", label: "Gemini 2.5 Flash", icon: "google" },
			{ id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", icon: "google", config: { reasoningEffort: "medium" } },
		],
	},
	"kimi-coding": {
		label: "Kimi",
		icon: "kimi-coding",
		summary: "Moonshot Kimi coding models through the provider's own API.",
		signupUrl: "https://platform.moonshot.ai/console/api-keys",
		apiKeyPlaceholder: "sk-...",
		models: [
			{ id: "kimi-for-coding", label: "Kimi For Coding", icon: "kimi" },
			{ id: "kimi-k2-thinking", label: "Kimi K2 Thinking", icon: "kimi" },
		],
	},
	deepseek: {
		label: "DeepSeek",
		icon: "deepseek",
		summary: "Direct DeepSeek API access for chat and reasoning models.",
		signupUrl: "https://platform.deepseek.com/api_keys",
		apiKeyPlaceholder: "sk-...",
		models: [
			{ id: "deepseek-chat", label: "DeepSeek Chat", icon: "deepseek" },
			{ id: "deepseek-reasoner", label: "DeepSeek Reasoner", icon: "deepseek" },
		],
	},
	"opencode-go": {
		label: "OpenCode Go",
		icon: "opencode-go",
		summary: "Custom OpenAI-compatible endpoint. Use this for self-hosted or gatewayed model APIs.",
		signupUrl: "https://opencode.ai",
		apiKeyPlaceholder: "api key",
		showBaseUrl: true,
		baseUrlLabel: "Endpoint",
		baseUrlPlaceholder: "https://opencode.ai/zen/go/v1",
		models: [
			{ id: "glm-5.1", label: "GLM-5.1", icon: "zhipuai", config: { reasoningEffort: "high", endpointKind: "openai-compatible" } },
			{ id: "glm-5", label: "GLM-5", icon: "zhipuai", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
			{ id: "kimi-k2.6", label: "Kimi K2.6", icon: "kimi", config: { reasoningEffort: "high", endpointKind: "openai-compatible" } },
			{ id: "kimi-k2.5", label: "Kimi K2.5", icon: "kimi", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
			{ id: "deepseek-v4-pro", label: "DeepSeek V4 Pro", icon: "deepseek", config: { reasoningEffort: "high", endpointKind: "openai-compatible" } },
			{ id: "deepseek-v4-flash", label: "DeepSeek V4 Flash", icon: "deepseek", config: { reasoningEffort: "low", endpointKind: "openai-compatible" } },
			{ id: "qwen3.6-plus", label: "Qwen3.6 Plus", icon: "qwen", config: { reasoningEffort: "medium", endpointKind: "anthropic" } },
			{ id: "qwen3.5-plus", label: "Qwen3.5 Plus", icon: "qwen", config: { reasoningEffort: "low", endpointKind: "anthropic" } },
			{ id: "mimo-v2.5-pro", label: "MiMo-V2.5-Pro", icon: "xiaomi", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
			{ id: "mimo-v2.5", label: "MiMo-V2.5", icon: "xiaomi", config: { reasoningEffort: "low", endpointKind: "openai-compatible" } },
			{ id: "mimo-v2-pro", label: "MiMo-V2-Pro", icon: "xiaomi", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
			{ id: "mimo-v2-omni", label: "MiMo-V2-Omni", icon: "xiaomi", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
			{ id: "minimax-m2.7", label: "MiniMax M2.7", icon: "minimax", config: { reasoningEffort: "low", endpointKind: "anthropic" } },
			{ id: "minimax-m2.5", label: "MiniMax M2.5", icon: "minimax", config: { reasoningEffort: "low", endpointKind: "anthropic" } },
			{ id: "hy3-preview", label: "HY3 Preview", icon: "hunyuan", config: { reasoningEffort: "medium", endpointKind: "openai-compatible" } },
		],
	},
};

export function orderedProviderNames(names: string[]): string[] {
	const order = Object.keys(PRESETS);
	return [...names].sort((a, b) => order.indexOf(a) - order.indexOf(b));
}
