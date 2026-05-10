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
		"relative inline-flex h-[20px] w-9 shrink-0 items-center rounded-full transition-colors duration-200",
		"focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2",
		checked ? "bg-foreground/85" : "bg-muted-foreground/25",
	)}
>
	<span
		class={cn(
			"inline-block size-3.5 rounded-full bg-white shadow-sm transition-transform duration-200",
			checked ? "translate-x-[18px]" : "translate-x-[3px]",
		)}
	></span>
</button>
