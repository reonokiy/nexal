<script lang="ts">
	import ChevronDown from "@lucide/svelte/icons/chevron-down";
	import X from "@lucide/svelte/icons/x";
	import { cn } from "$lib/utils";
	import { fade, fly } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

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

<div
	class="mx-auto flex w-full max-w-3xl flex-1 flex-col items-center justify-end pb-6"
	in:fade={{ duration: 220, easing: cubicOut }}
>
	<div
		class="mb-6 flex flex-col items-center"
		in:fly={{ y: 6, duration: 260, easing: cubicOut }}
	>
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
		<div
			class="w-full"
			in:fly={{ y: 8, duration: 240, easing: cubicOut, delay: 80 }}
			out:fade={{ duration: 160 }}
		>
			<div class="mb-1.5 flex justify-end">
				<button
					type="button"
					class="text-muted-foreground hover:text-foreground hover:bg-accent flex size-6 items-center justify-center rounded-full transition-colors"
					aria-label="dismiss suggestions"
					onclick={() => (dismissed = true)}
				>
					<X class="size-3.5" />
				</button>
			</div>
			<div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
				{#each SUGGESTIONS as s, i (s.text)}
					<button
						type="button"
						in:fly={{
							y: 8,
							duration: 240,
							easing: cubicOut,
							delay: 100 + i * 50,
						}}
						class={cn(
							"border-border bg-background hover:bg-accent/40",
							"flex flex-col gap-3 rounded-2xl border p-4 text-left transition-all duration-150 hover:-translate-y-0.5 active:translate-y-0",
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
