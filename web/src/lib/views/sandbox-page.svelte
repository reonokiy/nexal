<script lang="ts">
	import { onMount } from "svelte";
	import { Box, Server, RefreshCw, Circle } from "@lucide/svelte";

	interface Agent {
		agent_id: string;
		container_name: string;
		created_at_unix_ms: number;
	}

	let agents = $state<Agent[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let baseUrl = $state("");

	let interval: ReturnType<typeof setInterval> | undefined;

	function resolveBase(): string {
		if (typeof window === "undefined") return "";
		try {
			const raw = localStorage.getItem("nexal.backendUrl");
			if (raw) {
				const url: string = JSON.parse(raw);
				return url.replace(/^ws/, "http").replace(/:3001/, ":3000");
			}
		} catch { /* use fallback */ }
		return `${window.location.protocol}//${window.location.hostname}:3000`;
	}

	async function fetchAgents() {
		try {
			const url = `${baseUrl}/sandboxes`;
			const res = await fetch(url);
			if (!res.ok) throw new Error(`HTTP ${res.status}`);
			const data = await res.json();
			if (data.error) throw new Error(data.error);
			agents = data.agents ?? [];
			error = null;
		} catch (e) {
			error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		baseUrl = resolveBase();
		fetchAgents();
		interval = setInterval(fetchAgents, 10_000);
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
		return id.length > 12 ? id.slice(0, 12) + "…" : id;
	}
</script>

<div class="flex flex-col gap-6 p-6 max-w-4xl">
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-3">
			<Box class="size-5 text-muted-foreground" />
			<h1 class="text-lg font-semibold">Sandboxes</h1>
		</div>
		<button
			class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm
				bg-secondary text-secondary-foreground hover:bg-secondary/80 transition-colors"
			onclick={fetchAgents}
		>
			<RefreshCw class="size-3.5" />
			Refresh
		</button>
	</div>

	{#if loading}
		<div class="flex items-center gap-3 py-12 justify-center text-muted-foreground text-sm">
			<Circle class="size-4 animate-spin" />
			Loading sandboxes…
		</div>
	{:else if error}
		<div class="flex items-center gap-3 py-12 justify-center text-red-400 text-sm">
			<Server class="size-4" />
			{error}
		</div>
	{:else if agents.length === 0}
		<div class="flex flex-col items-center gap-2 py-16 text-muted-foreground">
			<Box class="size-8 opacity-30" />
			<p class="text-sm">No sandboxes running</p>
			<p class="text-xs opacity-55">Sandboxes are created when an agent spawns a worker</p>
		</div>
	{:else}
		<div class="rounded-lg border border-border overflow-hidden">
			<table class="w-full text-sm">
				<thead class="bg-muted/50">
					<tr>
						<th class="text-left px-4 py-2.5 font-medium text-muted-foreground">Container</th>
						<th class="text-left px-4 py-2.5 font-medium text-muted-foreground">Agent ID</th>
						<th class="text-left px-4 py-2.5 font-medium text-muted-foreground">Age</th>
						<th class="text-left px-4 py-2.5 font-medium text-muted-foreground">Status</th>
					</tr>
				</thead>
				<tbody>
					{#each agents as agent}
						<tr class="border-t border-border hover:bg-muted/30 transition-colors">
							<td class="px-4 py-2.5 font-mono text-xs">{agent.container_name}</td>
							<td class="px-4 py-2.5 font-mono text-xs text-muted-foreground">
								{shortId(agent.agent_id)}
							</td>
							<td class="px-4 py-2.5 text-muted-foreground">{formatAge(agent.created_at_unix_ms)}</td>
							<td class="px-4 py-2.5">
								<span class="inline-flex items-center gap-1.5">
									<span class="size-1.5 rounded-full bg-emerald-400 inline-block"></span>
									<span class="text-xs text-emerald-400">running</span>
								</span>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="text-xs text-muted-foreground text-right">
			{agents.length} sandbox{agents.length !== 1 ? "es" : ""} running
		</p>
	{/if}
</div>
