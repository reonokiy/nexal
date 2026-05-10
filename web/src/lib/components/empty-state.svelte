<script lang="ts">
	import ChevronDown from "@lucide/svelte/icons/chevron-down";
	import X from "@lucide/svelte/icons/x";
	import { cn } from "$lib/utils";

	interface Suggestion {
		emoji: string;
		text: string;
		fill: string;
	}

	interface Props {
		onPick: (text: string) => void;
	}
	let { onPick }: Props = $props();

	let dismissed = $state(false);

	const SUGGESTIONS: Suggestion[] = [
		{
			emoji: "🎮",
			text: "Build a classic Snake game in this repo.",
			fill: "Build a classic Snake game in this repo.",
		},
		{
			emoji: "📄",
			text: "Create a one-page summary that describes this app.",
			fill: "Create a one-page summary of this app.",
		},
		{
			emoji: "✏️",
			text: "Create a plan to ship the next milestone.",
			fill: "Create a plan to ",
		},
	];
</script>

<div class="mx-auto flex w-full max-w-3xl flex-1 flex-col items-center justify-end pb-6">
	<div class="mb-6 flex flex-col items-center">
		<div
			class="bg-foreground/[0.04] text-foreground/70 mb-5 flex size-12 items-center justify-center rounded-full"
		>
			<svg
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="1.7"
				stroke-linecap="round"
				stroke-linejoin="round"
				class="size-6"
				aria-hidden="true"
			>
				<path d="M5 8a4 4 0 0 1 4-4h6a4 4 0 0 1 4 4v3a4 4 0 0 1-4 4H9.5L6 18v-3.2A4 4 0 0 1 5 11Z" />
				<path d="M9 11h6" />
			</svg>
		</div>
		<h1 class="text-foreground text-3xl font-medium tracking-tight">
			Let's build
		</h1>
		<button
			type="button"
			class="text-muted-foreground hover:text-foreground mt-1 inline-flex items-center gap-1 text-3xl font-medium tracking-tight transition-colors"
		>
			<span>New project</span>
			<ChevronDown class="size-6 opacity-70" />
		</button>
	</div>

	{#if !dismissed}
		<div class="relative w-full">
			<button
				type="button"
				class="text-muted-foreground hover:bg-accent absolute -top-2 right-0 flex size-7 items-center justify-center rounded-full"
				aria-label="dismiss suggestions"
				onclick={() => (dismissed = true)}
			>
				<X class="size-4" />
			</button>
			<div class="grid grid-cols-1 gap-3 pt-2 sm:grid-cols-3">
				{#each SUGGESTIONS as s (s.text)}
					<button
						type="button"
						class={cn(
							"border-border bg-background hover:bg-accent/40",
							"flex flex-col gap-3 rounded-2xl border p-4 text-left transition-colors",
						)}
						onclick={() => onPick(s.fill)}
					>
						<span class="text-xl leading-none">{s.emoji}</span>
						<span class="text-foreground/85 text-sm leading-snug">
							{s.text}
						</span>
					</button>
				{/each}
			</div>
		</div>
	{/if}
</div>
