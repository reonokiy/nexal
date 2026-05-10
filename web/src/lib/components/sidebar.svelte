<script lang="ts">
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import SquarePen from "@lucide/svelte/icons/square-pen";
	import Search from "@lucide/svelte/icons/search";
	import Puzzle from "@lucide/svelte/icons/puzzle";
	import Clock from "@lucide/svelte/icons/clock";
	import Filter from "@lucide/svelte/icons/filter";
	import FolderPlus from "@lucide/svelte/icons/folder-plus";
	import Folder from "@lucide/svelte/icons/folder";
	import Settings from "@lucide/svelte/icons/settings";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	function newChat() {
		chat.messages.length = 0;
		router.go("home");
	}

	const navItems = [
		{ icon: SquarePen, label: "New chat", onclick: newChat, soon: false },
		{ icon: Search, label: "Search", onclick: () => {}, soon: true },
		{ icon: Puzzle, label: "Plugins", onclick: () => {}, soon: true },
		{ icon: Clock, label: "Automations", onclick: () => {}, soon: true },
	];
</script>

<aside
	class="border-border bg-muted/30 flex h-screen w-64 shrink-0 flex-col border-r"
>
	<nav class="flex flex-col gap-0.5 px-2 pt-3">
		{#each navItems as item (item.label)}
			<button
				type="button"
				disabled={item.soon}
				title={item.soon ? "Coming soon" : ""}
				class={cn(
					"flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150",
					item.soon
						? "text-muted-foreground/60 cursor-not-allowed"
						: "text-foreground/85 hover:bg-accent active:scale-[0.98]",
				)}
				onclick={item.onclick}
			>
				<item.icon class="size-4 transition-transform" />
				<span>{item.label}</span>
				{#if item.soon}
					<span
						class="bg-muted text-muted-foreground/70 ml-auto rounded px-1.5 text-[10px]"
					>
						soon
					</span>
				{/if}
			</button>
		{/each}
	</nav>

	<div class="mt-5 flex items-center justify-between px-3 py-1">
		<span class="text-muted-foreground text-xs">Threads</span>
		<div class="text-muted-foreground/50 flex items-center gap-1">
			<span
				class="cursor-not-allowed rounded p-1"
				title="Coming soon"
			>
				<Filter class="size-3.5" />
			</span>
			<span
				class="cursor-not-allowed rounded p-1"
				title="Coming soon"
			>
				<FolderPlus class="size-3.5" />
			</span>
		</div>
	</div>

	<div class="flex flex-col gap-0.5 px-2">
		<div
			class="text-foreground/85 flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm"
		>
			<Folder class="size-4" />
			<span>default</span>
		</div>
		<div class="text-muted-foreground px-2.5 py-1 text-xs">
			{chat.messages.filter((m) => m.role !== "system").length === 0
				? "No chats"
				: "current chat"}
		</div>
	</div>

	<div class="border-border mt-auto border-t px-2 py-2">
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
