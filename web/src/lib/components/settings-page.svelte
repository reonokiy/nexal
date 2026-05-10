<script lang="ts">
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { Badge } from "$lib/components/ui/badge";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import ArrowLeft from "@lucide/svelte/icons/arrow-left";
	import RefreshCw from "@lucide/svelte/icons/refresh-cw";
	import Eye from "@lucide/svelte/icons/eye";
	import EyeOff from "@lucide/svelte/icons/eye-off";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	interface ProviderInfo {
		name: string;
		hasKey: boolean;
		envKey: string | null;
	}
	interface ProvidersData {
		active: { provider: string; modelId: string } | null;
		providers: ProviderInfo[];
	}

	const DEFAULT_MODELS: Record<string, string> = {
		anthropic: "claude-sonnet-4-6",
		openai: "gpt-4o",
		openrouter: "openai/gpt-4o",
		google: "gemini-2.5-pro",
		mistral: "mistral-large-latest",
	};

	let providers = $state<ProviderInfo[]>([]);
	let active = $state<{ provider: string; modelId: string } | null>(null);
	let loading = $state(false);
	let loadError = $state<string | null>(null);

	// Per-provider editable form state (model id + new key + reveal flag).
	const form = $state<
		Record<string, { modelId: string; key: string; reveal: boolean; busy: boolean; flash: string | null }>
	>({});

	function ensureForm(name: string): void {
		if (form[name]) return;
		form[name] = {
			modelId:
				active?.provider === name
					? active.modelId
					: DEFAULT_MODELS[name] ?? "",
			key: "",
			reveal: false,
			busy: false,
			flash: null,
		};
	}

	async function refresh() {
		if (chat.status !== "open") {
			loadError = "Not connected to backend.";
			return;
		}
		loading = true;
		loadError = null;
		try {
			const res = await chat.runCommandAwait("providers", []);
			const data = res.data as ProvidersData | null | undefined;
			if (!data) {
				loadError = res.error ?? "providers command returned no data";
				return;
			}
			providers = data.providers;
			active = data.active;
			for (const p of providers) ensureForm(p.name);
		} catch (err) {
			loadError = (err as Error).message;
		} finally {
			loading = false;
		}
	}

	async function saveModel(name: string) {
		const f = form[name]!;
		if (!f.modelId.trim()) return;
		f.busy = true;
		try {
			await chat.runCommandAwait("model", [name, f.modelId.trim()]);
			f.flash = "model saved · restart nexal";
			setTimeout(() => (f.flash = null), 2500);
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	async function saveKey(name: string) {
		const f = form[name]!;
		if (!f.key.trim()) return;
		f.busy = true;
		try {
			await chat.runCommandAwait("apikey", [name, f.key.trim()]);
			f.key = "";
			f.flash = "key saved · restart nexal";
			setTimeout(() => (f.flash = null), 2500);
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	async function clearKey(name: string) {
		const f = form[name]!;
		f.busy = true;
		try {
			await chat.runCommandAwait("apikey", [name, "--clear"]);
			f.flash = "key cleared";
			setTimeout(() => (f.flash = null), 2500);
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	async function setActive(name: string) {
		const f = form[name]!;
		const modelId = f.modelId.trim() || DEFAULT_MODELS[name] || "";
		if (!modelId) return;
		await chat.runCommandAwait("model", [name, modelId]);
		await refresh();
	}

	onMount(() => {
		// Wait briefly if the socket is still connecting.
		const tryLoad = () => {
			if (chat.status === "open") void refresh();
			else setTimeout(tryLoad, 300);
		};
		tryLoad();
	});
</script>

<div class="bg-background text-foreground flex h-screen flex-1 flex-col">
	<header
		class="border-border flex h-12 items-center gap-3 border-b px-4"
	>
		<Button variant="ghost" size="sm" onclick={() => router.go("home")}>
			<ArrowLeft />
			back
		</Button>
		<span class="text-base font-semibold tracking-tight">Settings</span>
		<span class="text-muted-foreground text-xs">model providers</span>
		<div class="ml-auto flex items-center gap-2">
			<Button
				variant="outline"
				size="sm"
				onclick={refresh}
				disabled={loading}
			>
				<RefreshCw class={cn(loading && "animate-spin")} />
				refresh
			</Button>
		</div>
	</header>

	<main class="flex-1 overflow-y-auto px-4 py-6">
		<div class="mx-auto flex max-w-3xl flex-col gap-4">
			{#if active}
				<div
					class="border-border bg-muted/40 flex items-center gap-3 rounded-md border px-4 py-3 text-sm"
				>
					<span class="text-muted-foreground">active</span>
					<span class="font-mono">
						{active.provider} / {active.modelId}
					</span>
				</div>
			{:else}
				<div
					class="border-border bg-muted/40 text-muted-foreground rounded-md border px-4 py-3 text-sm"
				>
					No active model configured. Pick one below and press “use”.
				</div>
			{/if}

			{#if loadError}
				<div class="bg-destructive/10 text-destructive rounded-md p-3 text-sm">
					{loadError}
				</div>
			{/if}

			{#each providers as p (p.name)}
				{@const f = form[p.name]}
				{@const isActive = active?.provider === p.name}
				<section
					class={cn(
						"border-border rounded-lg border p-4",
						isActive && "border-primary/40 bg-primary/[0.03]",
					)}
				>
					<div class="mb-3 flex items-center gap-2">
						<h2 class="text-base font-semibold">{p.name}</h2>
						{#if isActive}
							<Badge variant="success">active</Badge>
						{/if}
						{#if p.hasKey}
							<Badge variant="secondary">key configured</Badge>
						{:else}
							<Badge variant="outline">no key</Badge>
						{/if}
						{#if p.envKey}
							<span class="text-muted-foreground ml-auto font-mono text-[10px]">
								env: {p.envKey}
							</span>
						{/if}
					</div>

					{#if f}
						<div class="grid grid-cols-[5rem_1fr_auto] items-center gap-2">
							<label for="model-{p.name}" class="text-xs">model id</label>
							<Input
								id="model-{p.name}"
								class="font-mono text-xs"
								placeholder={DEFAULT_MODELS[p.name] ?? "model id"}
								bind:value={f.modelId}
								disabled={f.busy}
							/>
							<div class="flex gap-2">
								<Button
									size="sm"
									variant="outline"
									onclick={() => saveModel(p.name)}
									disabled={f.busy || !f.modelId.trim()}
								>
									save
								</Button>
								<Button
									size="sm"
									onclick={() => setActive(p.name)}
									disabled={f.busy || isActive}
								>
									{isActive ? "active" : "use"}
								</Button>
							</div>

							<label for="key-{p.name}" class="text-xs">api key</label>
							<div class="relative">
								<Input
									id="key-{p.name}"
									type={f.reveal ? "text" : "password"}
									class="pr-9 font-mono text-xs"
									placeholder={p.hasKey ? "•••••••• (overwrite)" : "paste key here"}
									bind:value={f.key}
									disabled={f.busy}
								/>
								<button
									type="button"
									class="text-muted-foreground hover:text-foreground absolute right-2 top-1/2 -translate-y-1/2"
									aria-label={f.reveal ? "hide key" : "show key"}
									onclick={() => (f.reveal = !f.reveal)}
								>
									{#if f.reveal}<EyeOff class="size-4" />{:else}<Eye
											class="size-4"
										/>{/if}
								</button>
							</div>
							<div class="flex gap-2">
								<Button
									size="sm"
									variant="outline"
									onclick={() => clearKey(p.name)}
									disabled={f.busy || !p.hasKey}
								>
									clear
								</Button>
								<Button
									size="sm"
									onclick={() => saveKey(p.name)}
									disabled={f.busy || !f.key.trim()}
								>
									save
								</Button>
							</div>
						</div>

						{#if f.flash}
							<div class="text-primary mt-2 text-xs">{f.flash}</div>
						{/if}
					{/if}
				</section>
			{/each}

			<p class="text-muted-foreground text-xs">
				API keys are stored locally in <code>~/.nexal/data/</code> (PGlite).
				Changes take effect after restarting the nexal daemon.
			</p>
		</div>
	</main>
</div>
