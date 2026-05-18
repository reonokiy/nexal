<script lang="ts">
	import { tick, onMount } from "svelte";
	import { VList, type VListHandle } from "virtua/svelte";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";
	import Message from "$lib/components/message.svelte";
	import type { Source } from "$lib/components/message.svelte";
	import Composer from "$lib/components/composer.svelte";
	import EmptyState from "$lib/components/empty-state.svelte";
	import SandboxList from "$lib/components/sandbox-list.svelte";
	import MoreHorizontal from "@lucide/svelte/icons/more-horizontal";
	import PanelLeft from "@lucide/svelte/icons/panel-left";
	import Plug from "@lucide/svelte/icons/plug";
	import Box from "@lucide/svelte/icons/box";
	import { cn } from "$lib/utils";
	import { settings } from "$lib/settings.svelte";
	import type { Chat, Message as Msg } from "$lib/client.svelte";

	interface Props {
		chat: Chat;
		sidebarOpen: boolean;
		onToggleSidebar: () => void;
	}
	let { chat, sidebarOpen, onToggleSidebar }: Props = $props();

	let input = $state("");
	let modelLabel = $state("model");
	let showSandboxes = $state(false);

	interface MsgItem {
		id: number;
		streamId?: string;
		role: "user" | "agent";
		text: string;
		ts: number;
		streaming?: boolean;
		kind: "msg";
		source?: Source;
		toolName?: string;
		workerId?: string;
		workerStatus?: string;
	}

	type DisplayItem = MsgItem | { kind: "typing"; id: number };

	/** Parse source from message text (short-term until backend sends metadata). */
	function parseSource(text: string): {
		source?: Source;
		text: string;
		toolName?: string;
		workerId?: string;
		workerStatus?: string;
	} {
		const toolMatch = text.match(/^\[tool:(\w+)\]\s*/);
		if (toolMatch) {
			return {
				source: "tool",
				toolName: toolMatch[1],
				text: text.slice(toolMatch[0].length),
			};
		}
		const workerMatch = text.match(/^\[worker:([^\]]+)\]\s*status:(\w+)\s*/);
		if (workerMatch) {
			return {
				source: "worker",
				workerId: workerMatch[1],
				workerStatus: workerMatch[2],
				text: text.slice(workerMatch[0].length),
			};
		}
		return { text };
	}

	const visibleMessages = $derived(
		chat.messages.filter((m) => m.role !== "system"),
	);

	const displayItems = $derived.by<DisplayItem[]>(() => {
		const out: DisplayItem[] = visibleMessages.map((m) => {
			if (m.role === "agent") {
				const parsed = parseSource(m.text);
				return {
					id: m.id,
					streamId: m.streamId,
					role: m.role,
					text: parsed.text,
					ts: m.ts,
					streaming: m.streaming,
					kind: "msg" as const,
					source: parsed.source ?? "coordinator",
					toolName: parsed.toolName,
					workerId: parsed.workerId,
					workerStatus: parsed.workerStatus,
				};
			}
			// visibleMessages filters out system, so m.role must be "user" here
			return {
				id: m.id,
				streamId: m.streamId,
				role: m.role as "user",
				text: m.text,
				ts: m.ts,
				streaming: m.streaming,
				kind: "msg" as const,
			};
		});
		if (chat.typing && !chat.messages.some((m) => m.streaming)) {
			out.push({ kind: "typing", id: -1 });
		}
		return out;
	});

	const empty = $derived(
		chat.messages.filter((m) => m.role !== "system").length === 0,
	);

	const systemMessages = $derived(
		chat.messages.filter((m) => m.role === "system"),
	);

	let vlist: VListHandle | undefined = $state();
	let stickToBottom = $state(true);

	function onScroll(offset: number) {
		if (!vlist) return;
		const max = vlist.getScrollSize() - vlist.getViewportSize();
		stickToBottom = max - offset < 80;
	}

	$effect(() => {
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
			class="text-foreground/85 hover:bg-accent rounded-md px-2 py-1 text-sm font-medium transition-colors duration-150 active:scale-[0.97]"
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
					in:fade={{ duration: 150 }}
					out:fade={{ duration: 100 }}
					class="border-border hover:bg-accent flex items-center gap-1.5 rounded-md border px-3 py-1 text-sm transition-colors duration-150 active:scale-[0.97]"
					onclick={() => chat.connect(chat.url)}
					title="Reconnect to backend"
				>
					<Plug class="size-3.5" />
					reconnect
				</button>
			{/if}
			<button
				type="button"
				aria-label={showSandboxes ? "hide sandboxes" : "show sandboxes"}
				title={showSandboxes ? "Hide sandboxes" : "Show sandboxes"}
				class={cn(
					"flex size-8 items-center justify-center rounded-md transition-colors duration-150 active:scale-90",
					showSandboxes
						? "bg-accent text-foreground"
						: "text-muted-foreground hover:bg-accent",
				)}
				onclick={() => (showSandboxes = !showSandboxes)}
			>
				<Box class="size-4" />
			</button>
			<button
				type="button"
				aria-label={sidebarOpen ? "hide sidebar" : "show sidebar"}
				title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
				class="text-muted-foreground hover:bg-accent flex size-8 items-center justify-center rounded-md transition-colors duration-150 active:scale-90"
				onclick={onToggleSidebar}
			>
				<PanelLeft class="size-4" />
			</button>
		</div>
	</header>

	<!-- System messages banner -->
	{#if systemMessages.length > 0}
		<div class="bg-muted/30 border-b border-border px-4 py-1 flex items-center gap-2 text-[11px] text-muted-foreground">
			<span class="size-1.5 rounded-full bg-amber-400"></span>
			<span class="truncate">{systemMessages[systemMessages.length - 1]!.text}</span>
		</div>
	{/if}

	<main class="flex min-h-0 flex-1 flex-col">
		{#if showSandboxes}
			<div class="flex flex-1 overflow-y-auto">
				<SandboxList />
			</div>
		{:else if empty}
			<div
				class="flex flex-1 overflow-y-auto"
				in:fade={{ duration: 200, easing: cubicOut }}
				out:fade={{ duration: 120 }}
			>
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
						{:else}
							<Message
								role={item.role}
								source={item.source}
								text={item.text}
								ts={item.ts}
								streaming={item.streaming ?? false}
								toolName={item.toolName}
								workerId={item.workerId}
								workerStatus={item.workerStatus}
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
