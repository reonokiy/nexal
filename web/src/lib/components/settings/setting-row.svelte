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
		"flex items-center gap-4 px-5 py-3.5",
		!first && "border-border/60 border-t",
	)}
>
	<div class="min-w-0 flex-1">
		<div class="text-foreground text-sm font-medium leading-tight">
			{label}
		</div>
		{#if desc}
			<div class="text-muted-foreground mt-1 text-xs leading-snug">{desc}</div>
		{/if}
	</div>
	<div class="flex shrink-0 items-center gap-2">
		{@render children()}
	</div>
</div>
