<script lang="ts">
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Plus from "@lucide/svelte/icons/plus";
	import Mic from "@lucide/svelte/icons/mic";
	import ArrowUp from "@lucide/svelte/icons/arrow-up";
	import ChevronDown from "@lucide/svelte/icons/chevron-down";
	import Monitor from "@lucide/svelte/icons/monitor";
	import Sparkles from "@lucide/svelte/icons/sparkles";
	import type { Chat } from "$lib/client.svelte";

	interface Props {
		chat: Chat;
		value: string;
		onValueChange?: (v: string) => void;
		onSubmit: () => void;
		modelLabel?: string;
	}

	let {
		chat,
		value = $bindable(""),
		onSubmit,
		modelLabel = "model",
	}: Props = $props();

	let textareaEl: HTMLTextAreaElement | undefined = $state();

	function autoResize() {
		if (!textareaEl) return;
		textareaEl.style.height = "auto";
		textareaEl.style.height = Math.min(textareaEl.scrollHeight, 240) + "px";
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
			e.preventDefault();
			submit();
		}
	}

	function submit() {
		if (!value.trim()) return;
		onSubmit();
		queueMicrotask(autoResize);
	}

	$effect(() => {
		void value;
		autoResize();
	});
</script>

<div class="border-border bg-background rounded-2xl border shadow-sm">
	<textarea
		bind:this={textareaEl}
		bind:value
		oninput={autoResize}
		{onkeydown}
		placeholder="Ask nexal anything, / for commands"
		rows="1"
		class="text-foreground placeholder:text-muted-foreground/80 w-full resize-none bg-transparent px-4 pt-3.5 text-[15px] focus:outline-none"
		disabled={chat.status !== "open"}
	></textarea>

	<div class="flex items-center gap-1.5 px-2 pb-2 pt-1.5">
		<button
			type="button"
			disabled
			aria-label="attach (coming soon)"
			title="Coming soon"
			class="text-muted-foreground/50 flex size-8 cursor-not-allowed items-center justify-center rounded-full"
		>
			<Plus class="size-4" />
		</button>

		<button
			type="button"
			class="text-foreground/80 hover:bg-accent flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
			onclick={() => router.go("settings")}
			title="Configure model"
		>
			<Sparkles class="size-3.5" />
			<span>{modelLabel}</span>
			<ChevronDown class="size-3.5 opacity-60" />
		</button>

		<span
			class="text-muted-foreground/60 flex cursor-not-allowed items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
			title="Coming soon"
		>
			<span>Medium</span>
			<ChevronDown class="size-3.5 opacity-60" />
		</span>

		<div class="ml-auto flex items-center gap-1">
			<button
				type="button"
				disabled
				aria-label="voice input (coming soon)"
				title="Coming soon"
				class="text-muted-foreground/50 flex size-8 cursor-not-allowed items-center justify-center rounded-full"
			>
				<Mic class="size-4" />
			</button>
			<button
				type="button"
				aria-label="send"
				onclick={submit}
				disabled={chat.status !== "open" || !value.trim()}
				class={cn(
					"flex size-8 items-center justify-center rounded-full transition-all duration-150",
					"active:scale-90",
					value.trim() && chat.status === "open"
						? "bg-primary text-primary-foreground hover:opacity-90"
						: "bg-muted text-muted-foreground",
				)}
			>
				<ArrowUp class="size-4 transition-transform duration-150" />
			</button>
		</div>
	</div>
</div>

<div class="text-muted-foreground mt-1.5 flex items-center gap-3 px-2 text-xs">
	<span
		class="text-muted-foreground/60 flex cursor-not-allowed items-center gap-1 rounded-md px-2 py-1"
		title="Coming soon"
	>
		<Monitor class="size-3.5" />
		<span>Local</span>
		<ChevronDown class="size-3 opacity-60" />
	</span>
	<span class="ml-auto inline-flex items-center gap-1 rounded-md px-2 py-1">
		<span
			class={cn(
				"size-1.5 rounded-full transition-colors duration-200",
				chat.status === "open"
					? "bg-emerald-500"
					: chat.status === "connecting"
						? "bg-amber-500 animate-pulse"
						: "bg-rose-500",
			)}
		></span>
		<span class="transition-colors">{chat.status}</span>
	</span>
</div>
