<script lang="ts">
	import { tick, onMount } from "svelte";
	import { VList, type VListHandle } from "virtua/svelte";
	import Message from "$lib/components/message.svelte";
	import Composer from "$lib/components/composer.svelte";
	import EmptyState from "$lib/components/empty-state.svelte";
	import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
	import PanelLeft from "@lucide/svelte/icons/panel-left";
	import Plug from "@lucide/svelte/icons/plug";
	import type { Chat, Message as Msg } from "$lib/client.svelte";

	interface Props {
		chat: Chat;
		sidebarOpen: boolean;
		onToggleSidebar: () => void;
	}
	let { chat, sidebarOpen, onToggleSidebar }: Props = $props();

	let input = $state("");
	let modelLabel = $state("model");

	type DisplayItem =
		| (Msg & { kind: "msg" })
		| { kind: "typing"; id: number };

	const displayItems = $derived.by<DisplayItem[]>(() => {
		const out: DisplayItem[] = chat.messages.map((m) => ({
			...m,
			kind: "msg" as const,
		}));
		if (chat.typing && !chat.messages.some((m) => m.streaming)) {
			out.push({ kind: "typing", id: -1 });
		}
		return out;
	});

	const empty = $derived(
		chat.messages.filter((m) => m.role !== "system").length === 0,
	);

	let vlist: VListHandle | undefined = $state();
	let stickToBottom = $state(true);

	function onScroll(offset: number) {
		if (!vlist) return;
		const max = vlist.getScrollSize() - vlist.getViewportSize();
		stickToBottom = max - offset < 80;
	}

	$effect(() => {
		// Re-run on count or last item growth (streaming text) and typing flips.
		const last = displayItems[displayItems.length - 1];
		void displayItems.length;
		if (last && "text" in last) void last.text;
		if (!stickToBottom || !vlist || displayItems.length === 0) return;
		const idx = displayItems.length - 1;
		tick().then(() => vlist?.scrollToIndex(idx, { align: "end" }));
	});

	function send() {
		if (!input.trim()) return;
		chat.sendText(input);
		input = "";
		stickToBottom = true;
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
			// not connected yet — retry below
		}
	}

	onMount(() => {
		const tryLoad = () => {
			if (chat.status === "open") void refreshModelLabel();
			else setTimeout(tryLoad, 400);
		};
		tryLoad();
	});
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

	<main class="flex min-h-0 flex-1 flex-col">
		{#if empty}
			<div class="flex flex-1 overflow-y-auto">
				<EmptyState onPick={pickSuggestion} />
			</div>
		{:else}
			<VList
				bind:this={vlist}
				data={displayItems}
				getKey={(item: DisplayItem) => item.id}
				onscroll={onScroll}
				style="height: 100%; width: 100%;"
			>
				{#snippet children(item: DisplayItem)}
					<div class="mx-auto w-full max-w-3xl px-4">
						{#if item.kind === "typing"}
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
						{:else if item.role === "system"}
							<div class="text-muted-foreground py-2 text-center text-xs">
								{item.text}
							</div>
						{:else}
							<Message
								role={item.role}
								text={item.text}
								ts={item.ts}
								streaming={item.streaming ?? false}
							/>
						{/if}
					</div>
				{/snippet}
			</VList>
		{/if}
	</main>

	<div class="px-4 pb-4 pt-2">
		<div class="mx-auto w-full max-w-3xl">
			<Composer {chat} bind:value={input} onSubmit={send} {modelLabel} />
		</div>
	</div>
</div>
