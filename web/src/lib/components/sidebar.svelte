<script lang="ts">
	import { onMount } from "svelte";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Icon from "@iconify/svelte";
	import {
		billListLinear,
		boxLinear,
		chatRoundLineLinear,
		settingsLinear,
	} from "$lib/icons/solar";
	import type { Chat } from "$lib/client.svelte";

	interface Agent {
		agent_id: string;
		container_name: string;
		created_at_unix_ms: number;
	}

	interface TapeInfo {
		id: string;
		entries: number;
		anchors: number;
		lastAnchor: string | null;
		entriesSinceLastAnchor: number;
		lastTokenUsage: number | null;
	}

	let { chat }: { chat: Chat } = $props();

	let agents = $state<Agent[]>([]);
	let tapes = $state<TapeInfo[]>([]);
	let loadingComputers = $state(true);
	let loadingTapes = $state(true);
	let tapesError = $state<string | null>(null);

	let interval: ReturnType<typeof setInterval> | undefined;

	async function refreshAgents(silent = false) {
		if (chat.status !== "open") {
			if (!silent) {
				agents = [];
				loadingComputers = false;
			}
			return;
		}

		try {
			if (!silent) loadingComputers = true;
			const res = await chat.runCommandAwait("sandboxes", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as { agents?: Agent[] } | null | undefined;
			agents = data?.agents ?? [];
		} catch {
			if (!silent) agents = [];
		} finally {
			if (!silent) loadingComputers = false;
		}
	}

	async function refreshTapes(silent = false) {
		if (chat.status !== "open") {
			if (!silent) {
				tapes = [];
				tapesError = "Backend not connected";
				loadingTapes = false;
			}
			return;
		}

		try {
			if (!silent) loadingTapes = true;
			const res = await chat.runCommandAwait("tapes", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as { tapes?: TapeInfo[] } | null | undefined;
			tapes = data?.tapes ?? [];
			tapesError = null;
		} catch (e) {
			if (!silent) {
				tapes = [];
				tapesError = e instanceof Error ? e.message : "Unavailable";
			}
		} finally {
			if (!silent) loadingTapes = false;
		}
	}

	function shortTapeId(id: string): string {
		return id.length > 12 ? `${id.slice(0, 12)}...` : id;
	}

	onMount(() => {
		refreshAgents();
		refreshTapes();
		interval = setInterval(() => {
			refreshAgents(true);
			refreshTapes(true);
		}, 10_000);
		return () => clearInterval(interval);
	});
</script>

<aside
	class="border-border flex h-screen w-64 shrink-0 flex-col border-r bg-[#f3f3f3]"
>
	<div class="px-2 py-2 flex flex-col gap-0.5">
		<button
			type="button"
			class={cn(
				"text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
				router.current === "home"
					? "bg-black/[0.05] text-foreground"
					: "hover:bg-black/[0.04]",
			)}
			onclick={() => router.go("home")}
		>
			<Icon icon={chatRoundLineLinear} class="size-4" />
			<span>Chat</span>
		</button>
		<button
			type="button"
			class={cn(
				"text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
				router.current.startsWith("computers")
					? "bg-black/[0.05] text-foreground"
					: "hover:bg-black/[0.04]",
			)}
			onclick={() => router.go("computers")}
		>
			<Icon icon={boxLinear} class="size-4" />
			<span>Computers</span>
		</button>
		<div class="mt-1 flex flex-col gap-0.5 pl-8">
			{#if loadingComputers}
				<div class="text-muted-foreground flex items-center gap-2 px-2.5 py-1 text-xs">
					<span class="size-1.5 animate-pulse rounded-full bg-foreground/35"></span>
					<span class="animate-pulse">Loading…</span>
				</div>
			{:else if agents.length === 0}
				<div class="text-muted-foreground/70 px-2.5 py-1 text-xs">No computers</div>
			{:else}
				{#each agents as agent (agent.agent_id)}
					<button
						type="button"
						class={cn(
							"text-foreground/75 flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors duration-150",
							router.current === `computers/${agent.agent_id}`
								? "bg-black/[0.05] text-foreground"
								: "hover:bg-black/[0.04]",
						)}
						onclick={() => router.go(`computers/${agent.agent_id}`)}
						title={agent.container_name}
					>
						<span class="size-1.5 shrink-0 rounded-full bg-emerald-400"></span>
						<span class="truncate">{agent.container_name}</span>
					</button>
				{/each}
			{/if}
		</div>
		<button
			type="button"
			class={cn(
				"text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
				router.current === "tapes" || router.current === "taps"
					? "bg-black/[0.05] text-foreground"
					: "hover:bg-black/[0.04]",
			)}
			onclick={() => router.go("tapes")}
		>
			<Icon icon={billListLinear} class="size-4" />
			<span>Tapes</span>
		</button>
		<div class="mt-1 flex flex-col gap-0.5 pl-8">
			{#if loadingTapes}
				<div class="text-muted-foreground flex items-center gap-2 px-2.5 py-1 text-xs">
					<span class="size-1.5 animate-pulse rounded-full bg-foreground/35"></span>
					<span class="animate-pulse">Loading…</span>
				</div>
			{:else if tapesError}
				<div class="text-muted-foreground/70 px-2.5 py-1 text-xs" title={tapesError}>
					Tapes unavailable
				</div>
			{:else if tapes.length === 0}
				<div class="text-muted-foreground/70 px-2.5 py-1 text-xs">No tapes</div>
			{:else}
				{#each tapes as tape (tape.id)}
					<button
						type="button"
						class={cn(
							"text-foreground/75 flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors duration-150",
							router.current === `tapes/${tape.id}`
								? "bg-black/[0.05] text-foreground"
								: "hover:bg-black/[0.04]",
						)}
						onclick={() => router.go(`tapes/${encodeURIComponent(tape.id)}`)}
						title={tape.id}
					>
						<span class="min-w-0 flex-1 truncate">{shortTapeId(tape.id)}</span>
						<span class="text-muted-foreground/70 shrink-0">{tape.entries}</span>
					</button>
				{/each}
			{/if}
		</div>
	</div>

	<div class="flex-1"></div>

	<div class="mt-auto px-2 py-2 flex flex-col gap-0.5">
		<button
			type="button"
			class={cn(
				"text-foreground/85 flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
				router.current.startsWith("settings")
					? "bg-black/[0.05] text-foreground"
					: "hover:bg-black/[0.04]",
			)}
			onclick={() => router.go("settings")}
		>
			<Icon icon={settingsLinear} class="size-4" />
			<span>Settings</span>
		</button>
	</div>
</aside>
