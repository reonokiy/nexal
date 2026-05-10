<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		title?: string;
		children: Snippet;
	}
	let { title, children }: Props = $props();
</script>

{#if title}
	<h2 class="text-muted-foreground mt-6 mb-2 px-1 text-xs font-semibold uppercase tracking-wide">
		{title}
	</h2>
{/if}
<div class="border-border bg-background overflow-hidden rounded-2xl border">
	{@render children()}
</div>
