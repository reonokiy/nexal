<script lang="ts">
	import { tick } from "svelte";
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { Badge } from "$lib/components/ui/badge";
	import Message from "$lib/components/message.svelte";
	import { router } from "$lib/router.svelte";
	import Plug from "@lucide/svelte/icons/plug";
	import Send from "@lucide/svelte/icons/send";
	import Settings from "@lucide/svelte/icons/settings";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	let input = $state("");
	let scrollEl: HTMLDivElement | undefined = $state();

	$effect(() => {
		const last = chat.messages[chat.messages.length - 1];
		void chat.messages.length;
		void last?.text;
		void chat.typing;
		tick().then(() => {
			if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
		});
	});

	function submit(e: Event) {
		e.preventDefault();
		if (!input.trim()) return;
		chat.sendText(input);
		input = "";
	}

	const statusVariant = $derived(
		chat.status === "open"
			? "success"
			: chat.status === "connecting"
			  ? "warning"
			  : chat.status === "closed"
			    ? "destructive"
			    : "secondary",
	);
</script>

<div class="bg-background text-foreground flex h-screen flex-col">
	<header
		class="border-border bg-background/80 sticky top-0 z-10 flex items-center gap-3 border-b px-4 py-3 backdrop-blur"
	>
		<div class="flex items-center gap-2">
			<span class="text-lg font-semibold tracking-tight">nexal</span>
			<Badge variant={statusVariant as never}>{chat.status}</Badge>
		</div>
		<div class="ml-auto flex items-center gap-2">
			<Button
				variant="ghost"
				size="sm"
				onclick={() => router.go("settings")}
			>
				<Settings />
				settings
			</Button>
			<Input
				class="w-64 font-mono text-xs"
				placeholder="ws://host:port"
				bind:value={chat.url}
				onkeydown={(e) =>
					(e as KeyboardEvent).key === "Enter" && chat.connect(chat.url)}
			/>
			<Button size="sm" onclick={() => chat.connect(chat.url)}>
				<Plug />
				connect
			</Button>
		</div>
	</header>

	<main bind:this={scrollEl} class="flex-1 overflow-y-auto px-4 py-4">
		<div class="mx-auto flex max-w-3xl flex-col divide-y divide-border/60">
			{#each chat.messages as m (m.id)}
				{#if m.role === "system"}
					<div class="text-muted-foreground py-2 text-center text-xs">
						{m.text}
					</div>
				{:else}
					<Message
						role={m.role}
						text={m.text}
						ts={m.ts}
						streaming={m.streaming ?? false}
					/>
				{/if}
			{/each}
			{#if chat.typing && !chat.messages.some((m) => m.streaming)}
				<div class="flex items-center gap-1.5 py-3">
					<span class="bg-foreground/40 size-1.5 animate-bounce rounded-full"></span>
					<span
						class="bg-foreground/40 size-1.5 animate-bounce rounded-full"
						style="animation-delay: 0.15s"
					></span>
					<span
						class="bg-foreground/40 size-1.5 animate-bounce rounded-full"
						style="animation-delay: 0.3s"
					></span>
				</div>
			{/if}
		</div>
	</main>

	<form
		onsubmit={submit}
		class="border-border bg-background border-t px-4 py-3"
	>
		<div class="mx-auto flex max-w-3xl gap-2">
			<Input
				placeholder="说点什么 — 以 / 开头执行命令"
				bind:value={input}
				disabled={chat.status !== "open"}
				autocomplete="off"
			/>
			<Button
				type="submit"
				disabled={chat.status !== "open" || !input.trim()}
			>
				<Send />
				send
			</Button>
		</div>
	</form>
</div>
