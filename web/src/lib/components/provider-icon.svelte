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
	import openaiRaw from "simple-icons/icons/openaigym.svg?raw";
	import huggingfaceRaw from "simple-icons/icons/huggingface.svg?raw";
	import qwenRaw from "simple-icons/icons/qwen.svg?raw";
	import xiaomiRaw from "simple-icons/icons/xiaomi.svg?raw";
	import minimaxRaw from "simple-icons/icons/minimax.svg?raw";
	import qqRaw from "simple-icons/icons/qq.svg?raw";

	const ICONS: Record<string, string> = {
		openrouter: openrouterRaw,
		deepseek: deepseekRaw,
		"kimi-coding": moonshotRaw,
		"opencode-go": coderRaw,
		moonshot: moonshotRaw,
		anthropic: anthropicRaw,
		claude: claudeRaw,
		openai: openaiRaw,
		google: googleRaw,
		huggingface: huggingfaceRaw,
		qwen: qwenRaw,
		zhipuai: huggingfaceRaw,
		xiaomi: xiaomiRaw,
		minimax: minimaxRaw,
		hunyuan: qqRaw,
	};

	// Strip width/height/role/title from the SVG so we can re-style with
	// Tailwind classes and inherit `currentColor`.
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
	<span class={cn("inline-flex shrink-0 items-center justify-center", className)}>
		{@html html}
	</span>
{/if}
