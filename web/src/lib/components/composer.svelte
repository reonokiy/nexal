<script lang="ts">
	import { cn } from "$lib/utils";
	import Icon from "@iconify/svelte";
	import {
		addCircleLinear,
		altArrowDownLinear,
		arrowUpLinear,
		microphoneLinear,
		starsMinimalisticLinear,
	} from "$lib/icons/solar";
	import { settings } from "$lib/settings.svelte";
	import type { Chat } from "$lib/client.svelte";
	import { PRESETS, orderedProviderNames, type ModelOption } from "$lib/model-presets";
	import ProviderIcon from "$lib/components/provider-icon.svelte";

	interface Props {
		chat: Chat;
		value: string;
		onValueChange?: (v: string) => void;
		onSubmit: () => void;
		onModelChange?: () => void;
		modelLabel?: string;
	}

	interface ProviderInfo {
		name: string;
		hasKey: boolean;
	}

	interface ProvidersData {
		active: { provider: string; modelId: string } | null;
		providers: ProviderInfo[];
	}

	let {
		chat,
		value = $bindable(""),
		onSubmit,
		onModelChange,
		modelLabel = "model",
	}: Props = $props();

	let textareaEl: HTMLTextAreaElement | undefined = $state();
	let modelMenuOpen = $state(false);
	let loadingModels = $state(false);
	let modelError = $state<string | null>(null);
	let providers = $state<ProviderInfo[]>([]);
	let activeModel = $state<{ provider: string; modelId: string } | null>(null);
	let selectingModel = $state<string | null>(null);

	function autoResize() {
		if (!textareaEl) return;
		textareaEl.style.height = "auto";
		textareaEl.style.height = textareaEl.scrollHeight + "px";
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key !== "Enter" || e.isComposing) return;
		const wantShift = settings.sendKey === "shift-enter";
		if (wantShift) {
			if (!e.shiftKey) return; // plain Enter inserts a newline in this mode
		} else {
			if (e.shiftKey) return; // shift+Enter inserts a newline in plain-Enter mode
		}
		e.preventDefault();
		submit();
	}

	function submit() {
		if (!value.trim()) return;
		onSubmit();
		queueMicrotask(autoResize);
	}

	const configuredProviders = $derived.by(() => {
		const names = new Set<string>();
		for (const provider of providers) {
			if (provider.hasKey) names.add(provider.name);
		}
		if (activeModel) names.add(activeModel.provider);
		return orderedProviderNames([...names]).map((name) => ({
			name,
			preset: PRESETS[name],
		}));
	});

	function modelsFor(provider: string): ModelOption[] {
		const models = [...(PRESETS[provider]?.models ?? [])];
		const active = activeModel;
		if (active?.provider === provider && !models.some((model) => model.id === active.modelId)) {
			models.unshift({ id: active.modelId, label: active.modelId });
		}
		return models;
	}

	function modelIcon(provider: string, model: ModelOption): string {
		return model.icon ?? PRESETS[provider]?.icon ?? provider;
	}

	async function refreshModels() {
		if (chat.status !== "open") {
			modelError = "Backend not connected";
			return;
		}
		loadingModels = true;
		modelError = null;
		try {
			const res = await chat.runCommandAwait("providers", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as ProvidersData | null | undefined;
			providers = data?.providers ?? [];
			activeModel = data?.active ?? null;
		} catch (err) {
			modelError = err instanceof Error ? err.message : "Unable to load models";
			providers = [];
		} finally {
			loadingModels = false;
		}
	}

	async function toggleModelMenu() {
		modelMenuOpen = !modelMenuOpen;
		if (modelMenuOpen) await refreshModels();
	}

	async function selectModel(provider: string, modelId: string) {
		selectingModel = `${provider}/${modelId}`;
		try {
			const res = await chat.runCommandAwait("model", [provider, modelId]);
			if (res.error) throw new Error(res.error);
			activeModel = { provider, modelId };
			modelMenuOpen = false;
			onModelChange?.();
		} catch (err) {
			modelError = err instanceof Error ? err.message : "Unable to switch model";
		} finally {
			selectingModel = null;
		}
	}

	$effect(() => {
		void value;
		autoResize();
	});
</script>

<div class="border-border bg-background rounded-2xl border shadow-sm">
	<textarea
		bind:this={textareaEl}
		bind:value
		oninput={autoResize}
		{onkeydown}
		placeholder={settings.sendKey === "shift-enter"
			? "Message Coordinator · Shift+Enter to send"
			: "Message Coordinator · / for commands"}
		rows="1"
		class="text-foreground placeholder:text-muted-foreground/80 w-full resize-none bg-transparent px-4 pt-3 pb-1 text-[15px] focus:outline-none"
		disabled={chat.status !== "open"}
	></textarea>

	<div class="flex items-center gap-1.5 px-2 pb-2 pt-1.5">
		<button
			type="button"
			disabled
			aria-label="attach (coming soon)"
			title="Coming soon"
			class="text-muted-foreground/50 flex size-8 cursor-not-allowed items-center justify-center rounded-full"
		>
			<Icon icon={addCircleLinear} class="size-4" />
		</button>

		<div class="relative">
			<button
				type="button"
				class="text-foreground/80 hover:bg-accent flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
				onclick={toggleModelMenu}
				title="Choose model"
				aria-expanded={modelMenuOpen}
			>
				<Icon icon={starsMinimalisticLinear} class="size-3.5" />
				<span>{modelLabel}</span>
				<Icon icon={altArrowDownLinear} class="size-3.5 opacity-60" />
			</button>

			{#if modelMenuOpen}
				<div class="border-border bg-popover text-popover-foreground absolute bottom-8 left-0 z-20 w-72 overflow-hidden rounded-md border shadow-lg">
					<div class="border-border flex items-center justify-between gap-3 border-b px-3 py-2">
						<span class="text-sm font-medium">Models</span>
						<button
							type="button"
							class="text-muted-foreground hover:text-foreground text-xs"
							onclick={() => (modelMenuOpen = false)}
						>
							Close
						</button>
					</div>
					{#if loadingModels}
						<div class="text-muted-foreground px-3 py-4 text-sm">Loading models...</div>
					{:else if modelError}
						<div class="px-3 py-4 text-sm text-red-400">{modelError}</div>
					{:else if configuredProviders.length === 0}
						<div class="text-muted-foreground px-3 py-4 text-sm">No configured models.</div>
					{:else}
						<div class="max-h-80 overflow-y-auto py-1">
							{#each configuredProviders as provider (provider.name)}
								<div class="px-2 py-1">
									<div class="text-muted-foreground flex items-center gap-1.5 px-1 py-1 text-xs font-medium">
										<ProviderIcon name={provider.preset?.icon ?? provider.name} class="size-3.5" />
										<span>{provider.preset?.label ?? provider.name}</span>
									</div>
									{#each modelsFor(provider.name) as model (model.id)}
										{@const selected = activeModel?.provider === provider.name && activeModel.modelId === model.id}
										<button
											type="button"
											class={cn(
												"flex w-full items-center justify-between gap-2 rounded px-2 py-1.5 text-left text-sm transition-colors",
												selected ? "bg-accent text-foreground" : "hover:bg-accent/70",
											)}
											disabled={selectingModel !== null}
											onclick={() => selectModel(provider.name, model.id)}
										>
											<span class="flex min-w-0 items-center gap-2">
												<ProviderIcon name={modelIcon(provider.name, model)} class="size-4" />
												<span class="min-w-0">
													<span class="block truncate">{model.label}</span>
													<span class="text-muted-foreground block truncate text-xs">{model.id}</span>
												</span>
											</span>
											{#if selected}
												<span class="text-xs">active</span>
											{/if}
										</button>
									{/each}
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>

		<span
			class="text-muted-foreground/60 flex cursor-not-allowed items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
			title="Coming soon"
		>
			<span>Medium</span>
			<Icon icon={altArrowDownLinear} class="size-3.5 opacity-60" />
		</span>

		<div class="ml-auto flex items-center gap-1">
			<button
				type="button"
				disabled
				aria-label="voice input (coming soon)"
				title="Coming soon"
				class="text-muted-foreground/50 flex size-8 cursor-not-allowed items-center justify-center rounded-full"
			>
				<Icon icon={microphoneLinear} class="size-4" />
			</button>
			<button
				type="button"
				aria-label="send"
				onclick={submit}
				disabled={chat.status !== "open" || !value.trim()}
				class={cn(
					"flex size-8 items-center justify-center rounded-full transition-all duration-150",
					"active:scale-90",
					value.trim() && chat.status === "open"
						? "bg-primary text-primary-foreground hover:opacity-90"
						: "bg-muted text-muted-foreground",
				)}
			>
				<Icon icon={arrowUpLinear} class="size-4 transition-transform duration-150" />
			</button>
		</div>
	</div>
</div>
