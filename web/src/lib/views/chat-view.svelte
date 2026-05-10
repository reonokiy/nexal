<script lang="ts">
	import { tick, onMount } from "svelte";
	import Message from "$lib/components/message.svelte";
	import Composer from "$lib/components/composer.svelte";
	import EmptyState from "$lib/components/empty-state.svelte";
	import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
	import PanelLeft from "@lucide/svelte/icons/panel-left";
	import Plug from "@lucide/svelte/icons/plug";
	import type { Chat } from "$lib/client.svelte";

	interface Props {
		chat: Chat;
		sidebarOpen: boolean;
		onToggleSidebar: () => void;
	}
	let { chat, sidebarOpen, onToggleSidebar }: Props = $props();

	let input = $state("");
	let scrollEl: HTMLDivElement | undefined = $state();
	let modelLabel = $state("model");

	$effect(() => {
		const last = chat.messages[chat.messages.length - 1];
		void chat.messages.length;
		void last?.text;
		void chat.typing;
		tick().then(() => {
			if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
		});
	});

	function send() {
		if (!input.trim()) return;
		chat.sendText(input);
		input = "";
	}

	function pickSuggestion(text: string) {
		input = text;
	}

	function newChat() {
		chat.messages.length = 0;
		input = "";
	}

	async function refreshModelLabel() {
		try {
			const res = await chat.runCommandAwait("providers", []);
			const data = res.data as
				| { active: { provider: string; modelId: string } | null }
				| null
				| undefined;
			if (data?.active) {
				modelLabel = `${data.active.provider} / ${data.active.modelId}`;
			} else {
				modelLabel = "no model";
			}
		} catch {
			// connection isn't ready yet — try again on next status flip
		}
	}

	onMount(() => {
		const tryLoad = () => {
			if (chat.status === "open") void refreshModelLabel();
			else setTimeout(tryLoad, 400);
		};
		tryLoad();
	});

	const empty = $derived(
		chat.messages.filter((m) => m.role !== "system").length === 0,
	);
</script>

<div class="flex h-screen flex-1 flex-col">
	<header class="border-border flex h-12 items-center gap-2 border-b px-4">
		<button
			type="button"
			class="text-foreground/85 hover:bg-accent rounded-md px-2 py-1 text-sm font-medium"
			onclick={newChat}
			title="Clear chat"
		>
			New chat
		</button>
		<button
			type="button"
			disabled
			aria-label="thread menu (coming soon)"
			title="Coming soon"
			class="text-muted-foreground/50 flex size-7 cursor-not-allowed items-center justify-center rounded-md"
		>
			<MoreHorizontal class="size-4" />
		</button>
		<div class="ml-auto flex items-center gap-1">
			{#if chat.status !== "open"}
				<button
					type="button"
					class="border-border hover:bg-accent flex items-center gap-1.5 rounded-md border px-3 py-1 text-sm"
					onclick={() => chat.connect(chat.url)}
					title="Reconnect to backend"
				>
					<Plug class="size-3.5" />
					reconnect
				</button>
			{/if}
			<button
				type="button"
				aria-label={sidebarOpen ? "hide sidebar" : "show sidebar"}
				title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
				class="text-muted-foreground hover:bg-accent flex size-8 items-center justify-center rounded-md"
				onclick={onToggleSidebar}
			>
				<PanelLeft class="size-4" />
			</button>
		</div>
	</header>

	<main bind:this={scrollEl} class="flex flex-1 flex-col overflow-y-auto">
		{#if empty}
			<EmptyState onPick={pickSuggestion} />
		{:else}
			<div class="mx-auto flex w-full max-w-3xl flex-col px-4 py-6">
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
		{/if}
	</main>

	<div class="px-4 pb-4">
		<div class="mx-auto w-full max-w-3xl">
			<Composer {chat} bind:value={input} onSubmit={send} {modelLabel} />
		</div>
	</div>
</div>
