<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		title?: string;
		children: Snippet;
	}
	let { title, children }: Props = $props();
</script>

{#if title}
	<h2
		class="text-muted-foreground mb-2 mt-7 px-1 text-[11px] font-semibold uppercase tracking-wider"
	>
		{title}
	</h2>
{/if}
<div
	class="border-border/60 bg-background overflow-hidden rounded-xl border"
>
	{@render children()}
</div>
