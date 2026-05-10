<script lang="ts">
	import { renderMarkdown } from "$lib/markdown";
	import { cn } from "$lib/utils";
	import { settings } from "$lib/settings.svelte";

	interface Props {
		role: "user" | "agent";
		text: string;
		ts: number;
		streaming?: boolean;
	}
	let { role, text, ts, streaming = false }: Props = $props();

	const html = $derived(role === "agent" ? renderMarkdown(text) : "");

	function fmt(t: number) {
		const d = new Date(t);
		return `${d.getHours().toString().padStart(2, "0")}:${d
			.getMinutes()
			.toString()
			.padStart(2, "0")}`;
	}
</script>

<article
	class={cn("group flex flex-col gap-1", settings.compact ? "py-2" : "py-3")}
>
	<div class="text-muted-foreground flex items-baseline gap-2 text-xs">
		<span
			class={cn(
				"font-semibold",
				role === "user" ? "text-foreground" : "text-primary",
			)}
		>
			{role === "user" ? "you" : "nexal"}
		</span>
		{#if settings.showTimestamps}
			<time class="opacity-60">{fmt(ts)}</time>
		{/if}
		{#if streaming}
			<span class="text-primary inline-flex items-center gap-1 text-[10px]">
				<span class="bg-primary size-1.5 animate-pulse rounded-full"></span>
				streaming
			</span>
		{/if}
	</div>

	{#if role === "agent"}
		<div class="prose prose-sm md-body text-foreground max-w-none">
			{@html html}{#if streaming}<span class="md-cursor"></span>{/if}
		</div>
	{:else}
		<div class="text-foreground/90 text-sm leading-relaxed whitespace-pre-wrap break-words">
			{text}
		</div>
	{/if}
</article>
