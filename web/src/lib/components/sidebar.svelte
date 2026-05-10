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
		// Clear local transcript and ensure we're on the home route.
		chat.messages.length = 0;
		router.go("home");
	}

	const navItems = [
		{ icon: SquarePen, label: "New chat", onclick: newChat },
		{ icon: Search, label: "Search", onclick: () => {} },
		{ icon: Puzzle, label: "Plugins", onclick: () => {} },
		{ icon: Clock, label: "Automations", onclick: () => {} },
	];
</script>

<aside
	class="border-border bg-muted/30 flex h-screen w-64 shrink-0 flex-col border-r"
>
	<nav class="flex flex-col gap-0.5 px-2 pt-3">
		{#each navItems as item (item.label)}
			<button
				type="button"
				class="hover:bg-accent text-foreground/85 flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm"
				onclick={item.onclick}
			>
				<item.icon class="size-4" />
				<span>{item.label}</span>
			</button>
		{/each}
	</nav>

	<div class="mt-5 flex items-center justify-between px-3 py-1">
		<span class="text-muted-foreground text-xs">Threads</span>
		<div class="text-muted-foreground flex items-center gap-1">
			<button
				type="button"
				class="hover:bg-accent rounded p-1"
				aria-label="filter"
			>
				<Filter class="size-3.5" />
			</button>
			<button
				type="button"
				class="hover:bg-accent rounded p-1"
				aria-label="new folder"
			>
				<FolderPlus class="size-3.5" />
			</button>
		</div>
	</div>

	<div class="flex flex-col gap-0.5 px-2">
		<button
			type="button"
			class="hover:bg-accent text-foreground/85 flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm"
		>
			<Folder class="size-4" />
			<span>New project</span>
		</button>
		<div class="text-muted-foreground px-2.5 py-1 text-xs">
			{chat.messages.length === 0 ? "No chats" : "current chat"}
		</div>
	</div>

	<div class="mt-auto border-border border-t px-2 py-2">
		<button
			type="button"
			class={cn(
				"hover:bg-accent text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm",
				router.current === "settings" && "bg-accent",
			)}
			onclick={() => router.go("settings")}
		>
			<Settings class="size-4" />
			<span>Settings</span>
		</button>
	</div>
</aside>
