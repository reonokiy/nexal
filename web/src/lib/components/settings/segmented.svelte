<script lang="ts" generics="T extends string">
	import { cn } from "$lib/utils";

	interface Option {
		value: T;
		label: string;
	}

	interface Props {
		value: T;
		options: Option[];
		onchange: (v: T) => void;
	}

	let { value, options, onchange }: Props = $props();
</script>

<div
	class="bg-muted/60 inline-flex shrink-0 rounded-full p-[3px] text-xs"
>
	{#each options as opt (opt.value)}
		{@const active = value === opt.value}
		<button
			type="button"
			onclick={() => onchange(opt.value)}
			class={cn(
				"rounded-full px-3 py-1 transition-colors duration-150",
				active
					? "bg-background text-foreground shadow-sm"
					: "text-muted-foreground hover:text-foreground",
			)}
		>
			{opt.label}
		</button>
	{/each}
</div>
