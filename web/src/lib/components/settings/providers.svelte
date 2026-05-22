<script lang="ts">
	import { onMount } from "svelte";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import { cn } from "$lib/utils";
	import Icon from "@iconify/svelte";
	import {
		checkCircleLinear,
		eyeClosedLinear,
		eyeLinear,
		squareArrowRightUpLinear,
	} from "$lib/icons/solar";
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

	interface SettingsData {
		model: { provider: string; modelId: string } | null;
		providers: Array<{ name: string; base_url?: string }>;
		toolKeys: string[];
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
		apiKeyPlaceholder?: string;
		baseUrlPlaceholder?: string;
		baseUrlLabel?: string;
		showBaseUrl?: boolean;
		warn?: string;
	}

	const PRESETS: Record<string, ProviderPreset> = {
		openrouter: {
			label: "OpenRouter",
			icon: "openrouter",
			summary: "Single key for Claude, GPT, DeepSeek, Kimi and other hosted models.",
			signupUrl: "https://openrouter.ai/keys",
			apiKeyPlaceholder: "sk-or-...",
			models: [
				{ id: "openai/gpt-4o", label: "GPT-4o", hint: "general" },
				{ id: "anthropic/claude-sonnet-4-5", label: "Claude Sonnet 4.5", hint: "balanced" },
				{ id: "deepseek/deepseek-chat", label: "DeepSeek Chat", hint: "low cost" },
				{ id: "moonshotai/kimi-k2-thinking", label: "Kimi K2 Thinking", hint: "reasoning" },
			],
		},
		"kimi-coding": {
			label: "Kimi",
			icon: "kimi-coding",
			summary: "Moonshot Kimi coding models through the provider's own API.",
			signupUrl: "https://platform.moonshot.ai/console/api-keys",
			apiKeyPlaceholder: "sk-...",
			models: [
				{ id: "kimi-for-coding", label: "Kimi For Coding" },
				{ id: "kimi-k2-thinking", label: "Kimi K2 Thinking" },
			],
		},
		deepseek: {
			label: "DeepSeek",
			icon: "deepseek",
			summary: "Direct DeepSeek API access for chat and reasoning models.",
			signupUrl: "https://platform.deepseek.com/api_keys",
			apiKeyPlaceholder: "sk-...",
			models: [
				{ id: "deepseek-chat", label: "DeepSeek Chat" },
				{ id: "deepseek-reasoner", label: "DeepSeek Reasoner" },
			],
		},
		"opencode-go": {
			label: "OpenCode Go",
			icon: "opencode-go",
			summary: "Custom OpenAI-compatible endpoint. Use this for self-hosted or gatewayed model APIs.",
			signupUrl: "https://opencode.ai",
			apiKeyPlaceholder: "api key",
			showBaseUrl: true,
			baseUrlLabel: "Endpoint",
			baseUrlPlaceholder: "https://opencode.ai/zen/go/v1/chat/completions",
			models: [
				{ id: "kimi-k2.6", label: "Kimi K2.6", hint: "reasoning" },
				{ id: "kimi-k2.5", label: "Kimi K2.5", hint: "balanced" },
			],
		},
	};

	type ProviderForm = {
		modelId: string;
		key: string;
		reveal: boolean;
		busy: boolean;
		flash: string | null;
		baseUrl: string;
	};

	let providers = $state<ProviderInfo[]>([]);
	let active = $state<{ provider: string; modelId: string } | null>(null);
	let providerBaseUrls = $state<Record<string, string>>({});
	let loading = $state(false);
	let loadError = $state<string | null>(null);
	let selectedProviderName = $state<string>("");

	const form = $state<Record<string, ProviderForm>>({});

	function orderedProviders(items: ProviderInfo[]): ProviderInfo[] {
		const order = Object.keys(PRESETS);
		return [...items].sort((a, b) => order.indexOf(a.name) - order.indexOf(b.name));
	}

	function ensureForm(name: string): void {
		const preset = PRESETS[name];
		const defaultModel = preset?.models[0]?.id ?? "";
		const existing = form[name];
		if (!existing) {
			form[name] = {
				modelId: active?.provider === name ? active.modelId : defaultModel,
				key: "",
				reveal: false,
				busy: false,
				flash: null,
				baseUrl: providerBaseUrls[name] ?? "",
			};
			return;
		}
		if (active?.provider === name) existing.modelId = active.modelId;
		if (providerBaseUrls[name] !== undefined) existing.baseUrl = providerBaseUrls[name]!;
	}

	function setFlash(name: string, message: string, error = false) {
		const target = form[name];
		if (!target) return;
		target.flash = error ? `error: ${message}` : message;
		const current = target.flash;
		setTimeout(() => {
			if (form[name]?.flash === current) form[name]!.flash = null;
		}, error ? 3200 : 2400);
	}

	async function refresh() {
		if (chat.status !== "open") {
			loadError = "Not connected to backend.";
			providers = orderedProviders(
				Object.keys(PRESETS).map((name) => ({ name, hasKey: false, envKey: null })),
			);
			for (const provider of providers) ensureForm(provider.name);
			return;
		}
		loading = true;
		loadError = null;
		try {
			const [providersRes, settingsRes] = await Promise.allSettled([
				chat.runCommandAwait("providers", []),
				chat.runCommandAwait("settings", []),
			]);

			const providersData =
				providersRes.status === "fulfilled"
					? (providersRes.value.data as ProvidersData | null | undefined)
					: null;
			const settingsData =
				settingsRes.status === "fulfilled"
					? (settingsRes.value.data as SettingsData | null | undefined)
					: null;

			const fallbackProviders = Object.keys(PRESETS).map((name) => ({
				name,
				hasKey: false,
				envKey: null,
			}));

			const nextBaseUrls: Record<string, string> = {};
			for (const entry of settingsData?.providers ?? []) {
				nextBaseUrls[entry.name] = typeof entry.base_url === "string" ? entry.base_url : "";
			}

			providers = orderedProviders(providersData?.providers?.length ? providersData.providers : fallbackProviders);
			active = providersData?.active ?? settingsData?.model ?? null;
			providerBaseUrls = nextBaseUrls;

			for (const provider of providers) ensureForm(provider.name);

			const providerError =
				providersRes.status === "fulfilled" ? providersRes.value.error : providersRes.reason;
			const settingsError =
				settingsRes.status === "fulfilled" ? settingsRes.value.error : settingsRes.reason;
			if (providerError || settingsError) {
				loadError = String(providerError ?? settingsError);
			}
		} catch (err) {
			loadError = (err as Error).message;
			providers = orderedProviders(
				Object.keys(PRESETS).map((name) => ({ name, hasKey: false, envKey: null })),
			);
			for (const provider of providers) ensureForm(provider.name);
		} finally {
			loading = false;
		}
	}

	async function saveKey(name: string) {
		const entry = form[name]!;
		if (!entry.key.trim()) return;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("apikey", [name, entry.key.trim()]);
			if (res.error) throw new Error(res.error);
			entry.key = "";
			setFlash(name, "API key saved");
			await refresh();
		} catch (err) {
			setFlash(name, (err as Error).message, true);
		} finally {
			entry.busy = false;
		}
	}

	async function clearKey(name: string) {
		const entry = form[name]!;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("apikey", [name, "--clear"]);
			if (res.error) throw new Error(res.error);
			setFlash(name, "API key cleared");
			await refresh();
		} catch (err) {
			setFlash(name, (err as Error).message, true);
		} finally {
			entry.busy = false;
		}
	}

	async function saveBaseUrl(name: string) {
		const entry = form[name]!;
		const url = entry.baseUrl.trim();
		if (!url) return;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("settings", ["provider", name, "url", url]);
			if (res.error) throw new Error(res.error);
			setFlash(name, "Endpoint saved");
			await refresh();
		} catch (err) {
			setFlash(name, (err as Error).message, true);
		} finally {
			entry.busy = false;
		}
	}

	async function pickModel(name: string, modelId: string) {
		const entry = form[name]!;
		entry.modelId = modelId;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("model", [name, modelId]);
			if (res.error) throw new Error(res.error);
			setFlash(name, `Using ${modelId}`);
			await refresh();
		} catch (err) {
			setFlash(name, (err as Error).message, true);
		} finally {
			entry.busy = false;
		}
	}

	async function saveCustomModel(name: string) {
		const entry = form[name]!;
		const modelId = entry.modelId.trim();
		if (!modelId) return;
		entry.busy = true;
		try {
			const res = await chat.runCommandAwait("model", [name, modelId]);
			if (res.error) throw new Error(res.error);
			setFlash(name, `Using ${modelId}`);
			await refresh();
		} catch (err) {
			setFlash(name, (err as Error).message, true);
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

	$effect(() => {
		if (providers.length === 0) {
			selectedProviderName = "";
			return;
		}
		if (selectedProviderName && providers.some((provider) => provider.name === selectedProviderName)) {
			return;
		}
		selectedProviderName = active?.provider ?? providers[0]!.name;
	});

	const selectedProvider = $derived(
		providers.find((provider) => provider.name === selectedProviderName) ?? null,
	);
</script>

<section>
	<div class="grid gap-5 xl:grid-cols-[280px_minmax(0,1fr)]">
			<div class="rounded-2xl border border-border bg-background p-2">
				<div class="flex flex-col gap-1">
					{#each providers as provider (provider.name)}
						{@const preset = PRESETS[provider.name]}
						{@const isSelected = selectedProviderName === provider.name}
						<button
							type="button"
							class={cn(
								"flex w-full items-center gap-3 rounded-xl px-3 py-3 text-left transition-colors duration-150",
								isSelected
									? "bg-black/[0.05]"
									: "hover:bg-black/[0.03]",
							)}
							onclick={() => (selectedProviderName = provider.name)}
						>
							<span
								class={cn(
									"flex size-10 shrink-0 items-center justify-center rounded-xl border",
									provider.hasKey
										? "border-border bg-background"
										: "border-foreground/10 bg-foreground/[0.03]",
								)}
							>
								<ProviderIcon name={preset?.icon ?? provider.name} class="size-5" />
							</span>
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-2">
									<span class="truncate text-sm font-medium">{preset?.label ?? provider.name}</span>
									{#if active?.provider === provider.name}
										<span class="text-foreground/60 text-[10px]">active</span>
									{/if}
								</span>
								<span class="text-muted-foreground mt-0.5 block text-xs">
									{provider.hasKey ? "API key configured" : "API key required"}
								</span>
							</span>
						</button>
					{/each}
				</div>
			</div>

			{#if selectedProvider}
				{@const provider = selectedProvider}
				{@const preset = PRESETS[provider.name]}
				{@const entry = form[provider.name]}
				{@const isActive = active?.provider === provider.name}
				<section
					in:fly={{ y: 8, duration: 220, easing: cubicOut }}
					class={cn(
						"rounded-2xl border p-6 transition-colors",
						isActive && "border-foreground/20 bg-muted/15",
						!isActive && provider.hasKey && "border-border bg-background",
						!isActive && !provider.hasKey && "border-border/80 bg-gradient-to-br from-muted/45 via-background to-background",
					)}
				>
					{#if entry}
						<div class="flex items-start gap-4">
							<span
								class={cn(
									"flex size-12 shrink-0 items-center justify-center rounded-2xl border",
									provider.hasKey
										? "border-border bg-background"
										: "border-foreground/10 bg-foreground/[0.03]",
								)}
							>
								<ProviderIcon name={preset?.icon ?? provider.name} class="size-6" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<h3 class="text-lg font-medium">{preset?.label ?? provider.name}</h3>
									{#if isActive}
										<span class="border-foreground/20 text-foreground/70 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium">
											<Icon icon={checkCircleLinear} class="size-3" />
											active
										</span>
									{/if}
									{#if provider.hasKey}
										<span class="border-foreground/15 text-muted-foreground rounded-full border px-2 py-0.5 text-[10px]">
											ready
										</span>
									{:else}
										<span class="border-foreground/10 bg-foreground/[0.04] text-foreground/80 rounded-full border px-2 py-0.5 text-[10px]">
											no key
										</span>
									{/if}
								</div>
								<p class="text-muted-foreground mt-2 max-w-2xl text-sm leading-relaxed">
									{preset?.summary ?? "Configure this provider to use its models."}
								</p>
							</div>
							<a
								class="text-muted-foreground hover:text-foreground inline-flex h-9 items-center gap-1 rounded-md px-2 text-[11px]"
								href={preset?.signupUrl ?? "#"}
								target="_blank"
								rel="noreferrer"
							>
								get key
								<Icon icon={squareArrowRightUpLinear} class="size-3" />
							</a>
						</div>

						<div class="mt-6 grid gap-6 2xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
							<div class="flex flex-col gap-4">
								{#if loadError}
									<div class="border-destructive/30 bg-destructive/5 text-destructive rounded-xl border px-3 py-2 text-xs">
										{loadError}
									</div>
								{/if}
								<div class="space-y-1">
									<div class="text-muted-foreground text-[10px] uppercase tracking-wider">API key</div>
									<div class="relative">
										<Input
											type={entry.reveal ? "text" : "password"}
											class="h-10 pr-9 font-mono text-sm"
											placeholder={preset?.apiKeyPlaceholder ?? "paste API key"}
											bind:value={entry.key}
											disabled={entry.busy}
										/>
										<button
											type="button"
											class="text-muted-foreground hover:text-foreground absolute right-2 top-1/2 -translate-y-1/2"
											aria-label={entry.reveal ? "hide key" : "show key"}
											onclick={() => (entry.reveal = !entry.reveal)}
										>
											{#if entry.reveal}
												<Icon icon={eyeClosedLinear} class="size-4" />
											{:else}
												<Icon icon={eyeLinear} class="size-4" />
											{/if}
										</button>
									</div>
								</div>

								{#if preset?.showBaseUrl}
									<div class="space-y-1">
										<div class="text-muted-foreground text-[10px] uppercase tracking-wider">
											{preset.baseUrlLabel ?? "Base URL"}
										</div>
										<Input
											class="h-10 flex-1 font-mono text-sm"
											placeholder={preset.baseUrlPlaceholder ?? "https://..."}
											bind:value={entry.baseUrl}
											disabled={entry.busy}
										/>
									</div>
								{/if}

								<div class="flex flex-wrap gap-2 pt-1">
									<Button
										size="sm"
										class="h-9"
										onclick={() => saveKey(provider.name)}
										disabled={entry.busy || !entry.key.trim()}
									>
										save key
									</Button>
									{#if preset?.showBaseUrl}
										<Button
											variant="outline"
											size="sm"
											class="h-9"
											onclick={() => saveBaseUrl(provider.name)}
											disabled={entry.busy || !entry.baseUrl.trim()}
										>
											save endpoint
										</Button>
									{/if}
									{#if provider.hasKey}
										<Button
											variant="ghost"
											size="sm"
											class="h-9"
											onclick={() => clearKey(provider.name)}
											disabled={entry.busy}
										>
											clear
										</Button>
									{/if}
								</div>

								<div class="flex items-center justify-between gap-2 pt-1">
									<span class="text-muted-foreground font-mono text-[10px]">
										{provider.envKey ? `env: ${provider.envKey}` : provider.name}
									</span>
									{#if entry.flash}
										<span
											in:fly={{ y: 4, duration: 160, easing: cubicOut }}
											out:fade={{ duration: 140 }}
											class="text-foreground/70 text-xs"
										>
											{entry.flash}
										</span>
									{/if}
								</div>

								{#if preset?.warn}
									<div class="border-amber-500/30 bg-amber-500/[0.07] text-amber-700 dark:text-amber-400 rounded-md border px-2.5 py-2 text-[11px] leading-relaxed">
										{preset.warn}
									</div>
								{/if}
							</div>

							<div class="flex flex-col gap-4">
								<div class="min-w-0">
									<div class="text-sm font-medium">Model</div>
									<p class="text-muted-foreground mt-1 text-sm">
										{isActive && active ? active.modelId : "Choose the model this provider should run."}
									</p>
								</div>

								{#if !provider.hasKey}
									<div class="border-border/70 bg-background/70 rounded-xl border border-dashed px-3.5 py-3">
										<div class="flex items-start justify-between gap-3">
											<div class="min-w-0">
												<div class="text-sm font-medium">Model UI is ready</div>
												<p class="text-muted-foreground mt-1 text-xs leading-relaxed">
													You can preview the supported models now. Save the API key first to make this selection live.
												</p>
											</div>
											<div class="text-foreground/75 rounded-lg border px-2 py-1 text-[10px] uppercase tracking-wider">
												step 2
											</div>
										</div>
									</div>
								{/if}

								<div class="space-y-2">
									<div class="text-muted-foreground text-[11px] uppercase tracking-wider">Recommended models</div>
									<div class="flex flex-wrap gap-2">
										{#each preset?.models ?? [] as model (model.id)}
											{@const selected = active?.provider === provider.name && active.modelId === model.id}
											<button
												type="button"
												class={cn(
													"flex min-h-10 items-center gap-2 rounded-xl border px-3 py-2 text-left transition-colors disabled:opacity-50",
													selected
														? "border-foreground/40 bg-foreground/5"
														: "border-border hover:bg-accent",
												)}
												onclick={() => pickModel(provider.name, model.id)}
												disabled={entry.busy}
											>
												<div class="min-w-0">
													<div class="text-sm font-medium">{model.label}</div>
													<div class="text-muted-foreground font-mono text-[10px]">{model.id}</div>
												</div>
												{#if model.hint}
													<span class="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10px]">
														{model.hint}
													</span>
												{/if}
												{#if selected}
													<Icon icon={checkCircleLinear} class="size-3 shrink-0" />
												{/if}
											</button>
										{/each}
									</div>
								</div>

								<div class="space-y-1.5">
									<div class="text-muted-foreground text-[11px] uppercase tracking-wider">Custom model ID</div>
									<div class="flex items-center gap-2">
										<Input
											class="h-10 flex-1 font-mono text-sm"
											placeholder="custom model id"
											bind:value={entry.modelId}
											disabled={entry.busy}
										/>
										<Button
											variant="outline"
											size="sm"
											onclick={() => saveCustomModel(provider.name)}
											disabled={entry.busy || !entry.modelId.trim()}
										>
											use
										</Button>
									</div>
								</div>
							</div>
						</div>
					{/if}
				</section>
			{/if}
	</div>
</section>
