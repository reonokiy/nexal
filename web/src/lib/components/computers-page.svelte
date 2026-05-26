<script lang="ts">
	import { onMount } from "svelte";
	import { router } from "$lib/router.svelte";
	import Icon from "@iconify/svelte";
	import {
		boxLinear,
		chatRoundDotsLinear,
		clockCircleLinear,
		hashtagLinear,
		infoCircleLinear,
		recordCircleLinear,
		serverSquareLinear,
	} from "$lib/icons/solar";
	import type { Chat } from "$lib/client.svelte";

	interface Agent {
		agent_id: string;
		container_name: string;
		created_at_unix_ms: number;
	}

	let { chat }: { chat: Chat } = $props();

	let agents = $state<Agent[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let interval: ReturnType<typeof setInterval> | undefined;

	const selectedAgentId = $derived.by(() => {
		const match = router.current.match(/^computers\/(.+)$/);
		return match?.[1] ?? null;
	});

	const selectedAgent = $derived.by(() => {
		if (!selectedAgentId) return null;
		return agents.find((agent) => agent.agent_id === selectedAgentId) ?? null;
	});

	async function fetchAgents(silent = false) {
		if (chat.status !== "open") {
			if (!silent) {
				error = "Backend not connected";
				loading = false;
			}
			return;
		}

		try {
			if (!silent) loading = true;
			const res = await chat.runCommandAwait("sandboxes", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as { agents?: Agent[] } | null | undefined;
			agents = data?.agents ?? [];
			error = null;
		} catch (e) {
			if (!silent) error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			if (!silent) loading = false;
		}
	}

	onMount(() => {
		fetchAgents();
		interval = setInterval(() => fetchAgents(true), 10_000);
		return () => clearInterval(interval);
	});

	function formatAge(ms: number): string {
		const secs = Math.floor((Date.now() - ms) / 1000);
		if (secs < 60) return `${secs}s`;
		if (secs < 3600) return `${Math.floor(secs / 60)}m`;
		if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
		return `${Math.floor(secs / 86400)}d`;
	}

	function shortId(id: string): string {
		return id.length > 16 ? id.slice(0, 16) + "…" : id;
	}
</script>

<div class="mx-auto flex w-full max-w-5xl flex-1 flex-col gap-5 px-6 pb-5">
	{#if loading}
		<div class="flex flex-1 items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
			<Icon icon={recordCircleLinear} class="size-4 animate-spin" />
			<span class="animate-pulse">Loading computers…</span>
		</div>
	{:else if error}
		<div class="flex flex-1 items-center justify-center gap-2 py-12 text-sm text-red-400">
			<Icon icon={serverSquareLinear} class="size-4" />
			{error}
		</div>
	{:else if agents.length === 0}
		<div class="flex flex-1 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-border py-16 text-center">
			<Icon icon={boxLinear} class="size-8 opacity-35" />
			<div class="space-y-1">
				<p class="text-sm font-medium text-foreground/80">No computers running</p>
				<p class="text-xs text-muted-foreground">
					When a worker starts, it will appear in the Computers list on the left.
				</p>
			</div>
		</div>
	{:else if !selectedAgentId}
		<div class="flex flex-1 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-border py-16 text-center">
			<Icon icon={infoCircleLinear} class="size-8 opacity-35" />
			<div class="space-y-1">
				<p class="text-sm font-medium text-foreground/80">Select a computer</p>
				<p class="text-xs text-muted-foreground">
					Choose a running computer from the left sidebar to inspect it.
				</p>
			</div>
		</div>
	{:else if !selectedAgent}
		<div class="flex flex-1 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-border py-16 text-center">
			<Icon icon={serverSquareLinear} class="size-8 opacity-35" />
			<div class="space-y-1">
				<p class="text-sm font-medium text-foreground/80">Computer not available</p>
				<p class="text-xs text-muted-foreground">
					This computer may have stopped running. Pick another one from the left sidebar.
				</p>
			</div>
		</div>
	{:else}
		<div class="grid gap-5 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)]">
			<section class="rounded-2xl border border-border bg-background p-5">
				<div class="mb-4 flex items-center justify-between gap-3">
					<div>
						<h2 class="text-base font-medium">{selectedAgent.container_name}</h2>
						<p class="mt-1 text-xs text-muted-foreground">Connected computer details</p>
					</div>
					<span class="inline-flex items-center gap-1.5 rounded-full bg-emerald-500/10 px-2.5 py-1 text-xs text-emerald-600">
						<span class="size-1.5 rounded-full bg-emerald-500"></span>
						running
					</span>
				</div>

				<div class="grid gap-3 sm:grid-cols-2">
					<div class="rounded-xl border border-border bg-black/[0.015] p-3">
						<div class="mb-1 flex items-center gap-2 text-[11px] uppercase tracking-wider text-muted-foreground">
							<Icon icon={hashtagLinear} class="size-3.5" />
							Agent ID
						</div>
						<div class="font-mono text-sm">{shortId(selectedAgent.agent_id)}</div>
					</div>

					<div class="rounded-xl border border-border bg-black/[0.015] p-3">
						<div class="mb-1 flex items-center gap-2 text-[11px] uppercase tracking-wider text-muted-foreground">
							<Icon icon={clockCircleLinear} class="size-3.5" />
							Uptime
						</div>
						<div class="text-sm">{formatAge(selectedAgent.created_at_unix_ms)}</div>
					</div>
				</div>

				<div class="mt-4 rounded-xl border border-border bg-black/[0.015] p-4">
					<div class="mb-2 flex items-center gap-2 text-sm font-medium">
						<Icon icon={infoCircleLinear} class="size-4 text-muted-foreground" />
						Connection
					</div>
					<p class="text-sm leading-relaxed text-muted-foreground">
						This page is attached to the coordinator backend and is showing the selected
						running computer by its current gateway registration.
					</p>
				</div>
			</section>

			<section class="rounded-2xl border border-border bg-background p-5">
				<div class="mb-4 flex items-center gap-2">
					<Icon icon={chatRoundDotsLinear} class="size-4 text-muted-foreground" />
					<h2 class="text-base font-medium">Chat history</h2>
				</div>

				<div class="rounded-xl border border-dashed border-border bg-black/[0.015] p-4">
					<p class="text-sm font-medium text-foreground/80">History view is not wired yet</p>
					<p class="mt-1 text-sm leading-relaxed text-muted-foreground">
						The UI route is ready, but the backend does not yet expose per-computer chat
						history keyed by this agent id. Once that API exists, this panel can render the
						computer transcript here.
					</p>
				</div>
			</section>
		</div>
	{/if}
</div>
