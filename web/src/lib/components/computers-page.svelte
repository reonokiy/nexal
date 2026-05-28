<script lang="ts">
	import { onMount } from "svelte";
	import { router } from "$lib/router.svelte";
	import Icon from "@iconify/svelte";
	import {
		arrowUpLinear,
		boxLinear,
		clockCircleLinear,
		codeSquareLinear,
		hashtagLinear,
		infoCircleLinear,
		recordCircleLinear,
		serverSquareLinear,
	} from "$lib/icons/solar";
	import type { Chat } from "$lib/client.svelte";
	import type { Agent } from "$lib/computers.svelte";
	import { computers, refreshComputers, startComputersRefresh } from "$lib/computers.svelte";

	let { chat }: { chat: Chat } = $props();
	let startingSandbox = $state(false);
	let commandText = $state("");
	let runningCommand = $state(false);
	let terminalError = $state<string | null>(null);
	let shellHistory = $state<string[]>([]);
	let shellHistoryIndex = $state<number | null>(null);
	let shellHistoryDraft = $state("");
	let terminalEntries = $state<Array<{
		id: number;
		agentId: string;
		command: string;
		stdout: string;
		stderr: string;
		exitCode: number;
		timedOut: boolean;
		ts: number;
	}>>([]);
	let nextTerminalId = 1;

	const selectedAgentId = $derived.by(() => {
		const match = router.current.match(/^computers\/(.+)$/);
		return match?.[1] ?? null;
	});

	const selectedAgent = $derived.by(() => {
		if (!selectedAgentId) return null;
		return computers.agents.find((agent) => agent.agent_id === selectedAgentId) ?? null;
	});

	onMount(() => {
		return startComputersRefresh(chat);
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

	async function startManualSandbox() {
		if (startingSandbox || chat.status !== "open") return;
		try {
			startingSandbox = true;
			terminalError = null;
			const res = await chat.runCommandAwait("sandbox_start", [`manual-${Date.now().toString(36)}`], 60_000);
			if (res.error) throw new Error(res.error);
			const agent = (res.data as { agent?: Agent } | null | undefined)?.agent;
			await refreshComputers(chat, true);
			if (agent?.agent_id) router.go(`computers/${agent.agent_id}`);
		} catch (err) {
			terminalError = err instanceof Error ? err.message : "failed to start sandbox";
		} finally {
			startingSandbox = false;
		}
	}

	async function runSandboxCommand() {
		const agent = selectedAgent;
		const command = commandText.trim();
		if (!agent || !command || runningCommand || chat.status !== "open") return;
		try {
			runningCommand = true;
			terminalError = null;
			commandText = "";
			rememberShellCommand(command);
			const res = await chat.runCommandAwait("sandbox_exec", [agent.agent_id, command], 150_000);
			if (res.error) throw new Error(res.error);
			const data = res.data as {
				agentId?: string;
				command?: string;
				stdout?: string;
				stderr?: string;
				exitCode?: number;
				timedOut?: boolean;
			} | null | undefined;
			terminalEntries = [
				...terminalEntries,
				{
					id: nextTerminalId++,
					agentId: agent.agent_id,
					command: data?.command ?? command,
					stdout: data?.stdout ?? "",
					stderr: data?.stderr ?? "",
					exitCode: data?.exitCode ?? 0,
					timedOut: data?.timedOut ?? false,
					ts: Date.now(),
				},
			];
		} catch (err) {
			terminalError = err instanceof Error ? err.message : "command failed";
		} finally {
			runningCommand = false;
		}
	}

	function rememberShellCommand(command: string) {
		shellHistory = [...shellHistory.filter((item) => item !== command), command].slice(-100);
		shellHistoryIndex = null;
		shellHistoryDraft = "";
	}

	function recallShellHistory(direction: -1 | 1) {
		if (shellHistory.length === 0) return;
		if (shellHistoryIndex === null) {
			shellHistoryDraft = commandText;
			shellHistoryIndex = direction === -1 ? shellHistory.length - 1 : 0;
		} else {
			const nextIndex = shellHistoryIndex + direction;
			if (nextIndex < 0) {
				shellHistoryIndex = 0;
			} else if (nextIndex >= shellHistory.length) {
				shellHistoryIndex = null;
				commandText = shellHistoryDraft;
				return;
			} else {
				shellHistoryIndex = nextIndex;
			}
		}
		commandText = shellHistory[shellHistoryIndex] ?? shellHistoryDraft;
	}

	function onShellKeydown(event: KeyboardEvent) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || event.isComposing) return;
		if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
		event.preventDefault();
		recallShellHistory(event.key === "ArrowUp" ? -1 : 1);
	}
</script>

<div class="mx-auto flex w-full max-w-5xl flex-1 flex-col gap-5 px-6 pb-5">
	<div class="flex items-center justify-between gap-3">
		<div>
			<h1 class="text-base font-medium">Computers</h1>
			<p class="mt-1 text-xs text-muted-foreground">Run commands inside shared /workspace sandboxes.</p>
		</div>
		<button
			type="button"
			class="border-border hover:bg-black/[0.04] flex items-center gap-2 rounded-md border px-3 py-1.5 text-sm transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-60"
			onclick={startManualSandbox}
			disabled={startingSandbox || chat.status !== "open"}
			title={chat.status === "open" ? "Start a manual sandbox" : "Backend not connected"}
		>
			{#if startingSandbox}
				<Icon icon={recordCircleLinear} class="size-3.5 animate-spin" />
				<span>Starting</span>
			{:else}
				<Icon icon={boxLinear} class="size-3.5" />
				<span>Start sandbox</span>
			{/if}
		</button>
	</div>
	{#if terminalError}
		<div class="border-border rounded-md border bg-red-500/5 px-3 py-2 text-sm text-red-500">{terminalError}</div>
	{/if}
	{#if computers.loading && computers.agents.length === 0}
		<div class="flex flex-1 items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
			<Icon icon={recordCircleLinear} class="size-4 animate-spin" />
			<span class="animate-pulse">Loading computers…</span>
		</div>
	{:else if computers.error && computers.agents.length === 0}
		<div class="flex flex-1 items-center justify-center gap-2 py-12 text-sm text-red-400">
			<Icon icon={serverSquareLinear} class="size-4" />
			{computers.error}
		</div>
	{:else if computers.agents.length === 0}
		<div class="flex flex-1 flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-border py-16 text-center">
			<Icon icon={boxLinear} class="size-8 opacity-35" />
			<div class="space-y-1">
				<p class="text-sm font-medium text-foreground/80">No computers running</p>
				<p class="text-xs text-muted-foreground">
					Start a manual sandbox or wait for a worker to appear in the Computers list.
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
					<Icon icon={codeSquareLinear} class="size-4 text-muted-foreground" />
					<h2 class="text-base font-medium">Sandbox shell</h2>
				</div>

				<div class="flex min-h-80 flex-col rounded-xl border border-border bg-black/[0.015]">
					<div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
						{#if terminalEntries.filter((entry) => entry.agentId === selectedAgent.agent_id).length === 0}
							<div class="text-muted-foreground flex h-full min-h-40 items-center justify-center text-center text-sm">
								Run a command in /workspace to inspect or modify the shared filesystem.
							</div>
						{:else}
							{#each terminalEntries.filter((entry) => entry.agentId === selectedAgent.agent_id) as entry (entry.id)}
								<div class="rounded-md border border-border bg-background">
									<div class="border-border flex items-center justify-between gap-2 border-b px-3 py-2">
										<div class="min-w-0 truncate font-mono text-xs">$ {entry.command}</div>
										<div class="text-muted-foreground shrink-0 text-[11px]">
											exit {entry.exitCode}{entry.timedOut ? " · timeout" : ""}
										</div>
									</div>
									{#if entry.stdout}
										<pre class="whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs leading-relaxed">{entry.stdout}</pre>
									{/if}
									{#if entry.stderr}
										<pre class="border-border whitespace-pre-wrap break-words border-t px-3 py-2 font-mono text-xs leading-relaxed text-red-500">{entry.stderr}</pre>
									{/if}
									{#if !entry.stdout && !entry.stderr}
										<div class="text-muted-foreground px-3 py-2 text-xs">(no output)</div>
									{/if}
								</div>
							{/each}
						{/if}
					</div>
					<form class="border-border flex items-center gap-2 border-t p-2" onsubmit={(event) => { event.preventDefault(); void runSandboxCommand(); }}>
						<input
							class="placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent px-2 py-1.5 font-mono text-sm outline-none"
							bind:value={commandText}
							oninput={() => {
								shellHistoryIndex = null;
								shellHistoryDraft = "";
							}}
							placeholder="ls -la /workspace"
							disabled={runningCommand || chat.status !== "open"}
							onkeydown={onShellKeydown}
						/>
						<button
							type="submit"
							class="bg-foreground text-background flex size-8 items-center justify-center rounded-md transition-opacity duration-150 disabled:cursor-not-allowed disabled:opacity-50"
							disabled={runningCommand || chat.status !== "open" || !commandText.trim()}
							title="Run command"
						>
							{#if runningCommand}
								<Icon icon={recordCircleLinear} class="size-3.5 animate-spin" />
							{:else}
								<Icon icon={arrowUpLinear} class="size-3.5" />
							{/if}
						</button>
					</form>
				</div>
			</section>
		</div>
	{/if}
</div>
