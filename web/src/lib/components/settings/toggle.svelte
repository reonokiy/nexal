<script lang="ts">
	import { cn } from "$lib/utils";

	interface Props {
		checked: boolean;
		onchange: (v: boolean) => void;
		label?: string;
	}
	let { checked, onchange, label }: Props = $props();
</script>

<button
	type="button"
	role="switch"
	aria-checked={checked}
	aria-label={label}
	onclick={() => onchange(!checked)}
	class={cn(
		"relative inline-flex h-6 w-10 shrink-0 items-center rounded-full transition-colors duration-200",
		"focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2",
		checked ? "bg-primary" : "bg-muted",
	)}
>
	<span
		class={cn(
			"bg-background inline-block size-4 rounded-full shadow-sm transition-transform duration-200",
			checked ? "translate-x-5" : "translate-x-1",
		)}
	></span>
</button>
