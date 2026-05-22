<script lang="ts">
	import { onMount } from "svelte";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import Icon from "@iconify/svelte";
	import { eyeClosedLinear, eyeLinear } from "$lib/icons/solar";
	import type { Chat } from "$lib/client.svelte";
	import { fade } from "svelte/transition";

	let { chat }: { chat: Chat } = $props();

	interface SettingsData {
		toolKeys: string[];
	}

	const TOOL_KEYS = ["tavily", "jina", "gemini"] as const;

	type ToolForm = {
		key: string;
		reveal: boolean;
		busy: boolean;
		flash: string | null;
	};

	let configuredToolKeys = $state<string[]>([]);
	let loading = $state(false);
	let loadError = $state<string | null>(null);

	const toolForm = $state<Record<string, ToolForm>>({});

	function ensureTool(name: string): void {
		if (toolForm[name]) return;
		toolForm[name] = { key: "", reveal: false, busy: false, flash: null };
	}

	for (const key of TOOL_KEYS) ensureTool(key);

	function setToolFlash(name: string, message: string, error = false) {
		const target = toolForm[name];
		if (!target) return;
		target.flash = error ? `error: ${message}` : message;
		const current = target.flash;
		setTimeout(() => {
			if (toolForm[name]?.flash === current) toolForm[name]!.flash = null;
		}, error ? 3200 : 2400);
	}

	async function refresh() {
		if (chat.status !== "open") {
			loadError = "Not connected to backend.";
			return;
		}
		loading = true;
		loadError = null;
		try {
			const settingsRes = await chat.runCommandAwait("settings", []);
			const settingsData = settingsRes.data as SettingsData | null | undefined;
			configuredToolKeys = settingsData?.toolKeys ?? [];
		} catch (err) {
			loadError = (err as Error).message;
		} finally {
			loading = false;
		}
	}

	async function saveToolKey(name: string) {
		const entry = toolForm[name]!;
		if (!entry.key.trim()) return;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("settings", ["toolkey", name, entry.key.trim()]);
			if (res.error) throw new Error(res.error);
			entry.key = "";
			configuredToolKeys = [...new Set([...configuredToolKeys, name])];
			setToolFlash(name, "saved");
		} catch (err) {
			setToolFlash(name, (err as Error).message, true);
		} finally {
			entry.busy = false;
		}
	}

	onMount(() => {
		const tryLoad = () => {
			if (chat.status === "open") void refresh();
			else setTimeout(tryLoad, 300);
		};
		tryLoad();
	});
</script>

<section>
	<div class="flex items-baseline gap-3">
		<h1 class="text-foreground text-2xl font-medium tracking-tight">Tool API keys</h1>
		{#if loading}
			<span
				in:fade={{ duration: 150 }}
				out:fade={{ duration: 100 }}
				class="text-muted-foreground/70 text-xs"
			>
				syncing…
			</span>
		{/if}
	</div>
	<p class="text-muted-foreground mt-1 text-sm">
		Configure optional external tool providers used by the workspace.
	</p>

	{#if loadError}
		<div
			in:fade={{ duration: 150 }}
			out:fade={{ duration: 100 }}
			class="border-destructive/40 bg-destructive/5 text-destructive mt-4 rounded-lg border px-3 py-2 text-sm"
		>
			{loadError}
		</div>
	{/if}

	<div class="mt-8 flex flex-col gap-4">
		{#each TOOL_KEYS as name (name)}
			{@const entry = toolForm[name]}
			<section class="rounded-2xl border border-border bg-background p-5">
				<div class="flex items-center justify-between gap-3">
					<div>
						<h2 class="text-base font-medium uppercase">{name}</h2>
						<p class="text-muted-foreground mt-1 text-sm">
							Save the API key for the `{name}` tool integration.
						</p>
					</div>
					{#if configuredToolKeys.includes(name)}
						<span class="border-foreground/15 text-muted-foreground rounded-full border px-2 py-0.5 text-[10px]">
							configured
						</span>
					{/if}
				</div>

				<div class="mt-4 flex items-center gap-2">
					<div class="relative flex-1">
						<Input
							type={entry.reveal ? "text" : "password"}
							class="h-10 pr-9 font-mono text-sm"
							placeholder="paste key"
							bind:value={entry.key}
							disabled={entry.busy}
						/>
						<button
							type="button"
							class="text-muted-foreground hover:text-foreground absolute right-2 top-1/2 -translate-y-1/2"
							onclick={() => (entry.reveal = !entry.reveal)}
						>
							{#if entry.reveal}
								<Icon icon={eyeClosedLinear} class="size-4" />
							{:else}
								<Icon icon={eyeLinear} class="size-4" />
							{/if}
						</button>
					</div>
					<Button size="sm" class="h-10" onclick={() => saveToolKey(name)} disabled={entry.busy || !entry.key.trim()}>
						save
					</Button>
				</div>

				{#if entry.flash}
					<div class="text-foreground/70 mt-3 text-xs">{entry.flash}</div>
				{/if}
			</section>
		{/each}
	</div>
</section>
