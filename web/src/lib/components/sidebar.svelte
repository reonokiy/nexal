<script lang="ts">
	import { onMount } from "svelte";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Icon from "@iconify/svelte";
	import {
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

	let { chat }: { chat: Chat } = $props();

	let agents = $state<Agent[]>([]);
	let loadingComputers = $state(true);

	let interval: ReturnType<typeof setInterval> | undefined;

	async function refreshAgents() {
		if (chat.status !== "open") {
			agents = [];
			loadingComputers = false;
			return;
		}

		try {
			loadingComputers = true;
			const res = await chat.runCommandAwait("sandboxes", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as { agents?: Agent[] } | null | undefined;
			agents = data?.agents ?? [];
		} catch {
			agents = [];
		} finally {
			loadingComputers = false;
		}
	}

	onMount(() => {
		refreshAgents();
		interval = setInterval(refreshAgents, 10_000);
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
