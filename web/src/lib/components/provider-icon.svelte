<script lang="ts" module>
	// Inline brand SVGs from `simple-icons`. `?raw` returns the raw file
	// contents at build time, so the bundle ships only the icons we use.
	import openrouterRaw from "simple-icons/icons/openrouter.svg?raw";
	import deepseekRaw from "simple-icons/icons/deepseek.svg?raw";
	import moonshotRaw from "simple-icons/icons/moonshotai.svg?raw";
	import anthropicRaw from "simple-icons/icons/anthropic.svg?raw";
	import claudeRaw from "simple-icons/icons/claude.svg?raw";
	import coderRaw from "simple-icons/icons/coder.svg?raw";
	import googleRaw from "simple-icons/icons/google.svg?raw";
	import googleGeminiRaw from "simple-icons/icons/googlegemini.svg?raw";
	import huggingfaceRaw from "simple-icons/icons/huggingface.svg?raw";
	import qwenRaw from "simple-icons/icons/qwen.svg?raw";
	import xiaomiRaw from "simple-icons/icons/xiaomi.svg?raw";
	import minimaxRaw from "simple-icons/icons/minimax.svg?raw";
	import qqRaw from "simple-icons/icons/qq.svg?raw";

	const openaiRaw =
		'<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M22.281 9.821a5.984 5.984 0 0 0-.515-4.91 6.046 6.046 0 0 0-6.509-2.9A6.065 6.065 0 0 0 4.981 4.18a5.984 5.984 0 0 0-3.998 2.9 6.046 6.046 0 0 0 .742 7.097 5.98 5.98 0 0 0 .515 4.911 6.051 6.051 0 0 0 6.51 2.9A5.984 5.984 0 0 0 13.259 24a6.052 6.052 0 0 0 5.772-4.206 5.984 5.984 0 0 0 3.997-2.9 6.052 6.052 0 0 0-.747-7.073zM13.26 22.43a4.476 4.476 0 0 1-2.876-1.04l.141-.081 4.779-2.759a.794.794 0 0 0 .392-.681v-6.737l2.02 1.168a.071.071 0 0 1 .038.052v5.583a4.504 4.504 0 0 1-4.494 4.494zM3.6 18.304a4.47 4.47 0 0 1-.535-3.015l.142.085 4.783 2.759a.771.771 0 0 0 .78 0l5.843-3.369v2.332a.08.08 0 0 1-.033.062L9.74 19.95a4.499 4.499 0 0 1-6.14-1.646zM2.34 7.896a4.485 4.485 0 0 1 2.366-1.973v5.676a.766.766 0 0 0 .387.676l5.814 3.354-2.02 1.168a.076.076 0 0 1-.071 0l-4.83-2.787A4.504 4.504 0 0 1 2.34 7.896zm16.597 3.855-5.843-3.37 2.02-1.167a.076.076 0 0 1 .071 0l4.83 2.786a4.504 4.504 0 0 1-.676 8.104v-5.677a.766.766 0 0 0-.402-.676zm2.011-3.037-.142-.085-4.773-2.782a.771.771 0 0 0-.78 0L9.41 9.217V6.885a.08.08 0 0 1 .033-.062l4.83-2.787a4.504 4.504 0 0 1 6.675 4.678zM8.306 12.863l-2.02-1.168a.071.071 0 0 1-.038-.052V6.061a4.504 4.504 0 0 1 7.371-3.453l-.142.081-4.778 2.759a.794.794 0 0 0-.393.681v6.734zm1.102-2.365L12 9.003l2.591 1.495v2.99L12 14.984l-2.592-1.496v-2.99z"/></svg>';
	const glmRaw =
		'<svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><rect x="2.5" y="2.5" width="19" height="19" rx="5" fill="currentColor"/><path d="M7.2 15.65c-1.82 0-3.05-1.29-3.05-3.22s1.3-3.26 3.18-3.26c1.13 0 2.07.46 2.62 1.28l-1.1.78c-.35-.48-.82-.72-1.48-.72-1.02 0-1.68.75-1.68 1.91 0 1.2.64 1.92 1.68 1.92.76 0 1.24-.34 1.39-.98H7.28v-1.18h2.88v3.34H8.97l-.08-.71c-.38.55-.94.84-1.69.84Zm4.05-.13V9.31h1.48v4.88h2.7v1.33h-4.18Zm5.01 0V9.31h1.72l1.35 3.8 1.35-3.8h1.72v6.21h-1.38v-4.02l-1.22 3.42h-.95l-1.22-3.42v4.02h-1.37Z" fill="white"/></svg>';

	const ICONS: Record<string, string> = {
		openrouter: openrouterRaw,
		deepseek: deepseekRaw,
		"kimi-coding": moonshotRaw,
		kimi: moonshotRaw,
		"opencode-go": coderRaw,
		moonshot: moonshotRaw,
		anthropic: anthropicRaw,
		claude: claudeRaw,
		openai: openaiRaw,
		google: googleRaw,
		gemini: googleGeminiRaw,
		huggingface: huggingfaceRaw,
		qwen: qwenRaw,
		zhipuai: glmRaw,
		xiaomi: xiaomiRaw,
		minimax: minimaxRaw,
		hunyuan: qqRaw,
	};

	const COLORS: Record<string, string> = {
		openrouter: "#111827",
		deepseek: "#4D6BFE",
		"kimi-coding": "#111827",
		kimi: "#111827",
		"opencode-go": "#111827",
		moonshot: "#111827",
		anthropic: "#CC785C",
		claude: "#D97757",
		openai: "#10A37F",
		google: "#4285F4",
		gemini: "#8E75B2",
		huggingface: "#FFD21E",
		qwen: "#5A47FF",
		zhipuai: "#2454FF",
		xiaomi: "#FF6900",
		minimax: "#111827",
		hunyuan: "#12B7F5",
	};

	// Strip dimensions and metadata while preserving each brand's own fill color.
	function clean(svg: string): string {
		return svg
			.replace(/<title>[^<]*<\/title>/g, "")
			.replace(/\s(?:width|height|role)="[^"]*"/g, "")
			.replace("<svg", '<svg fill="currentColor"');
	}

	const CLEANED: Record<string, string> = Object.fromEntries(
		Object.entries(ICONS).map(([k, v]) => [k, clean(v)]),
	);
</script>

<script lang="ts">
	import { cn } from "$lib/utils";

	interface Props {
		name: string;
		class?: string;
	}
	let { name, class: className }: Props = $props();

	const html = $derived(CLEANED[name] ?? "");
</script>

{#if html}
	<span
		class={cn("inline-flex shrink-0 items-center justify-center", className)}
		style:color={COLORS[name] ?? "currentColor"}
	>
		{@html html}
	</span>
{/if}
