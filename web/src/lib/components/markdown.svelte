<script lang="ts">
	import SvelteMarkdown from "@humanspeak/svelte-markdown";
	import type { Renderers } from "@humanspeak/svelte-markdown";
	import MarkdownCode from "$lib/components/markdown-code.svelte";
	import { cn } from "$lib/utils";

	interface Props {
		source: string;
		streaming?: boolean;
		class?: string;
	}

	let { source, streaming = false, class: className }: Props = $props();

	const renderers: Partial<Renderers> = {
		code: MarkdownCode,
	};
</script>

<div class={cn("md-body text-foreground max-w-none", className)}>
	<SvelteMarkdown {source} {streaming} {renderers} />
	{#if streaming}<span class="md-cursor"></span>{/if}
</div>
