<script lang="ts">
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Settings from "@lucide/svelte/icons/settings";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();
</script>

<aside
	class="border-border bg-muted/30 flex h-screen w-64 shrink-0 flex-col border-r"
>
	<div class="px-3 pt-3 pb-2">
		<div class="flex items-center gap-2 px-2.5 py-2">
			<span class="bg-primary/10 text-primary text-[10px] font-semibold rounded px-1.5 py-0.5 uppercase tracking-wider">Coordinator</span>
			<span
				class={cn(
					"size-2 rounded-full",
					chat.status === "open"
						? "bg-emerald-400"
						: chat.status === "connecting"
							? "bg-amber-400 animate-pulse"
							: "bg-rose-400",
				)}
			></span>
			<span class="text-muted-foreground text-[10px]">{chat.status}</span>
		</div>
		<p class="text-muted-foreground/60 text-[11px] px-2.5">
			All messages route through the coordinator.
		</p>
	</div>

	<div class="flex-1"></div>

	<div class="border-border mt-auto border-t px-2 py-2 flex flex-col gap-0.5">
		<button
			type="button"
			class={cn(
				"hover:bg-accent text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
				router.current.startsWith("settings") && "bg-accent",
			)}
			onclick={() => router.go("settings")}
		>
			<Settings class="size-4" />
			<span>Settings</span>
		</button>
	</div>
</aside>
