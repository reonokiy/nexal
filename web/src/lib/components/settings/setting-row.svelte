<script lang="ts">
	import { cn } from "$lib/utils";
	import type { Snippet } from "svelte";

	interface Props {
		label: string;
		desc?: string;
		first?: boolean;
		children: Snippet;
	}
	let { label, desc, first = false, children }: Props = $props();
</script>

<div
	class={cn(
		"flex items-center gap-4 px-4 py-4",
		!first && "border-border border-t",
	)}
>
	<div class="min-w-0 flex-1">
		<div class="text-foreground text-sm font-medium">{label}</div>
		{#if desc}
			<div class="text-muted-foreground mt-0.5 text-xs">{desc}</div>
		{/if}
	</div>
	<div class="flex shrink-0 items-center gap-2">
		{@render children()}
	</div>
</div>
