<script lang="ts">
	import { onMount } from "svelte";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Eye from "@lucide/svelte/icons/eye";
	import EyeOff from "@lucide/svelte/icons/eye-off";
	import Check from "@lucide/svelte/icons/check";
	import ExternalLink from "@lucide/svelte/icons/external-link";
	import ProviderIcon from "$lib/components/provider-icon.svelte";
	import type { Chat } from "$lib/client.svelte";
	import { fade, fly } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

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
	interface ModelOption {
		id: string;
		label: string;
		hint?: string;
	}
	interface ProviderPreset {
		label: string;
		icon: string;
		summary: string;
		signupUrl: string;
		models: ModelOption[];
		warn?: string;
	}

	const PRESETS: Record<string, ProviderPreset> = {
		openrouter: {
			label: "OpenRouter",
			icon: "openrouter",
			summary: "Single key, 100+ models. Works for Claude, GPT, DeepSeek, Kimi…",
			signupUrl: "https://openrouter.ai/keys",
			models: [
				{ id: "openai/gpt-4o", label: "GPT-4o", hint: "general" },
				{
					id: "anthropic/claude-sonnet-4-5",
					label: "Claude Sonnet 4.5",
					hint: "balanced",
				},
				{
					id: "deepseek/deepseek-chat",
					label: "DeepSeek Chat",
					hint: "low cost",
				},
				{
					id: "moonshotai/kimi-k2-thinking",
					label: "Kimi K2 Thinking",
					hint: "reasoning",
				},
			],
		},
		"kimi-coding": {
			label: "Kimi",
			icon: "kimi-coding",
			summary: "Moonshot Kimi for Coding — Anthropic-compatible API.",
			signupUrl: "https://www.kimi.com/coding",
			models: [
				{ id: "kimi-for-coding", label: "Kimi For Coding" },
				{ id: "kimi-k2-thinking", label: "Kimi K2 Thinking" },
			],
		},
		deepseek: {
			label: "DeepSeek",
			icon: "deepseek",
			summary: "Direct DeepSeek API. Save the key now; switch via OpenRouter to use it today.",
			signupUrl: "https://platform.deepseek.com/api_keys",
			models: [
				{ id: "deepseek-chat", label: "DeepSeek Chat" },
				{ id: "deepseek-reasoner", label: "DeepSeek Reasoner" },
			],
			warn: "Not natively routed by pi-ai yet — pick it under OpenRouter for now.",
		},
	};

	let providers = $state<ProviderInfo[]>([]);
	let active = $state<{ provider: string; modelId: string } | null>(null);
	let loading = $state(false);
	let loadError = $state<string | null>(null);

	const form = $state<
		Record<
			string,
			{
				modelId: string;
				key: string;
				reveal: boolean;
				busy: boolean;
				flash: string | null;
			}
		>
	>({});

	function ensureForm(name: string): void {
		if (form[name]) return;
		const preset = PRESETS[name];
		const def = preset?.models[0]?.id ?? "";
		form[name] = {
			modelId: active?.provider === name ? active.modelId : def,
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

	function flash(name: string, msg: string) {
		const f = form[name];
		if (!f) return;
		f.flash = msg;
		setTimeout(() => {
			if (form[name]?.flash === msg) form[name]!.flash = null;
		}, 2500);
	}

	async function saveKey(name: string) {
		const f = form[name]!;
		if (!f.key.trim()) return;
		f.busy = true;
		try {
			await chat.runCommandAwait("apikey", [name, f.key.trim()]);
			f.key = "";
			flash(name, "key saved · restart nexal");
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
			flash(name, "key cleared");
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	async function pickModel(name: string, modelId: string) {
		const f = form[name]!;
		f.modelId = modelId;
		f.busy = true;
		try {
			await chat.runCommandAwait("model", [name, modelId]);
			flash(name, `using ${modelId} · restart nexal`);
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	async function saveCustomModel(name: string) {
		const f = form[name]!;
		const id = f.modelId.trim();
		if (!id) return;
		f.busy = true;
		try {
			await chat.runCommandAwait("model", [name, id]);
			flash(name, `using ${id} · restart nexal`);
			await refresh();
		} finally {
			f.busy = false;
		}
	}

	onMount(() => {
		const tryLoad = () => {
			if (chat.status === "open") void refresh();
			else setTimeout(tryLoad, 300);
		};
		tryLoad();
	});

	const inputCls = "h-9 text-sm";
	const labelCls = "text-muted-foreground w-16 shrink-0 pt-2 text-xs";
</script>

<div class="bg-background text-foreground flex h-screen flex-1 flex-col">
	<header class="border-border flex h-12 items-center gap-3 border-b px-4">
		<span class="text-foreground/85 px-1 text-sm font-medium">Settings</span>
		<span class="text-muted-foreground text-xs">model providers</span>
		{#if loading}
			<span
				in:fade={{ duration: 150 }}
				out:fade={{ duration: 100 }}
				class="text-muted-foreground/70 ml-2 text-xs"
			>
				syncing…
			</span>
		{/if}
	</header>

	<main class="flex-1 overflow-y-auto px-4 py-6">
		<div class="mx-auto flex w-full max-w-3xl flex-col gap-4">
			<div>
				<h1 class="text-foreground text-2xl font-medium tracking-tight">
					Model providers
				</h1>
				<p class="text-muted-foreground mt-1 text-sm">
					Save an API key for each subscription you use, then pick a model.
					Changes apply after the daemon restarts.
				</p>
			</div>

			{#if active}
				<div
					in:fade={{ duration: 200 }}
					class="border-border bg-muted/40 flex h-10 items-center gap-2 rounded-lg border px-3 text-sm transition-colors"
				>
					<span class="text-muted-foreground text-xs">active</span>
					<span class="font-mono text-xs">
						{active.provider} / {active.modelId}
					</span>
				</div>
			{/if}

			{#if loadError}
				<div
					in:fade={{ duration: 150 }}
					out:fade={{ duration: 100 }}
					class="border-destructive/40 bg-destructive/5 text-destructive rounded-lg border px-3 py-2 text-sm"
				>
					{loadError}
				</div>
			{/if}

			{#each providers as p, i (p.name)}
				{@const preset = PRESETS[p.name]}
				{@const f = form[p.name]}
				{@const isActive = active?.provider === p.name}
				<section
					in:fly={{ y: 8, duration: 240, easing: cubicOut, delay: i * 60 }}
					class={cn(
						"border-border rounded-2xl border p-5 transition-colors duration-200",
						isActive && "border-foreground/30 bg-muted/20",
					)}
				>
					<div class="mb-4 flex items-start gap-3">
						<span
							class="border-border bg-background text-foreground/85 flex size-9 shrink-0 items-center justify-center rounded-lg border"
						>
							<ProviderIcon
								name={preset?.icon ?? p.name}
								class="size-5"
							/>
						</span>
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-center gap-2">
								<h2 class="text-base font-medium">
									{preset?.label ?? p.name}
								</h2>
								{#if isActive}
									<span
										class="border-foreground/20 text-foreground/70 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium"
									>
										<Check class="size-3" />
										active
									</span>
								{/if}
								{#if p.hasKey}
									<span
										class="border-foreground/15 text-muted-foreground rounded-full border px-2 py-0.5 text-[10px]"
									>
										key configured
									</span>
								{:else}
									<span
										class="bg-muted text-muted-foreground rounded-full px-2 py-0.5 text-[10px]"
									>
										no key
									</span>
								{/if}
							</div>
							{#if preset}
								<p class="text-muted-foreground mt-1 text-sm">
									{preset.summary}
								</p>
							{/if}
						</div>
						{#if preset}
							<a
								class="text-muted-foreground hover:text-foreground inline-flex h-9 items-center gap-1 rounded-md px-2 text-xs"
								href={preset.signupUrl}
								target="_blank"
								rel="noreferrer"
							>
								get key
								<ExternalLink class="size-3" />
							</a>
						{/if}
					</div>

					{#if preset?.warn}
						<div
							class="border-amber-500/30 bg-amber-500/[0.07] text-amber-700 dark:text-amber-400 mb-4 rounded-md border px-3 py-2 text-xs"
						>
							{preset.warn}
						</div>
					{/if}

					{#if f}
						<div class="flex flex-col gap-3">
							<!-- API key row -->
							<div class="flex items-start gap-2">
								<label
									for="key-{p.name}"
									class={labelCls}
								>
									api key
								</label>
								<div class="relative flex-1">
									<Input
										id="key-{p.name}"
										type={f.reveal ? "text" : "password"}
										class={cn(inputCls, "pr-9 font-mono")}
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
										{#if f.reveal}
											<EyeOff class="size-4" />
										{:else}
											<Eye class="size-4" />
										{/if}
									</button>
								</div>
								{#if p.hasKey}
									<Button
										variant="ghost"
										size="sm"
										onclick={() => clearKey(p.name)}
										disabled={f.busy}
									>
										clear
									</Button>
								{/if}
								<Button
									size="sm"
									onclick={() => saveKey(p.name)}
									disabled={f.busy || !f.key.trim()}
								>
									save
								</Button>
							</div>

							<!-- Models row -->
							{#if preset?.models.length}
								<div class="flex items-start gap-2">
									<span class={labelCls}>models</span>
									<div class="flex flex-1 flex-wrap gap-1.5">
										{#each preset.models as m (m.id)}
											{@const selected =
												active?.provider === p.name && active.modelId === m.id}
											<button
												type="button"
												class={cn(
													"flex h-9 items-center gap-2 rounded-md border px-3 text-xs transition-colors disabled:opacity-50",
													selected
														? "border-foreground/40 bg-foreground/5"
														: "border-border hover:bg-accent",
												)}
												onclick={() => pickModel(p.name, m.id)}
												disabled={f.busy}
											>
												<span class="font-medium">{m.label}</span>
												<span class="text-muted-foreground font-mono text-[10px]">
													{m.id}
												</span>
												{#if m.hint}
													<span
														class="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10px]"
													>
														{m.hint}
													</span>
												{/if}
												{#if selected}
													<Check class="size-3" />
												{/if}
											</button>
										{/each}
									</div>
								</div>
							{/if}

							<!-- Custom model row -->
							<div class="flex items-start gap-2">
								<label for="model-{p.name}" class={labelCls}>custom</label>
								<Input
									id="model-{p.name}"
									class={cn(inputCls, "flex-1 font-mono")}
									placeholder="custom model id"
									bind:value={f.modelId}
									disabled={f.busy}
								/>
								<Button
									variant="outline"
									size="sm"
									onclick={() => saveCustomModel(p.name)}
									disabled={f.busy || !f.modelId.trim()}
								>
									use
								</Button>
							</div>

							<div class="flex items-center justify-between gap-2 pt-1">
								{#if p.envKey}
									<span class="text-muted-foreground font-mono text-[10px]">
										env: {p.envKey}
									</span>
								{:else}
									<span></span>
								{/if}
								{#if f.flash}
									<span
										in:fly={{ y: 4, duration: 180, easing: cubicOut }}
										out:fade={{ duration: 150 }}
										class="text-foreground/70 text-xs"
									>
										{f.flash}
									</span>
								{/if}
							</div>
						</div>
					{/if}
				</section>
			{/each}

			<p class="text-muted-foreground text-xs">
				API keys are stored locally in <code>~/.nexal/data/</code> (PGlite).
			</p>
		</div>
	</main>
</div>
