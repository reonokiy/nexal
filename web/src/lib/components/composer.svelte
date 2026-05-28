<script lang="ts">
	import { onMount } from "svelte";
	import { cn } from "$lib/utils";
	import Icon from "@iconify/svelte";
	import {
		addCircleLinear,
		altArrowDownLinear,
		arrowUpLinear,
		cpuLinear,
		microphoneLinear,
	} from "$lib/icons/solar";
	import { settings } from "$lib/settings.svelte";
	import type { Chat } from "$lib/client.svelte";
	import type { CommandInfo } from "@nexal/transport";
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

	interface ModelCache extends ProvidersData {
		updatedAt: number;
	}

	const MODEL_CACHE_KEY = "nexal.modelPicker";
	const COMMAND_HISTORY_KEY = "nexal.commandHistory";
	const MAX_COMMAND_HISTORY = 100;

	let {
		chat,
		value = $bindable(""),
		onSubmit,
		onModelChange,
		modelLabel = "model",
	}: Props = $props();

	let textareaEl: HTMLTextAreaElement | undefined = $state();
	let modelMenuEl: HTMLDivElement | undefined = $state();
	let modelMenuOpen = $state(false);
	let loadingModels = $state(false);
	let modelError = $state<string | null>(null);
	let hasModelCache = $state(false);
	let providers = $state<ProviderInfo[]>([]);
	let activeModel = $state<{ provider: string; modelId: string } | null>(null);
	let selectingModel = $state<string | null>(null);
	let commands = $state<CommandInfo[]>([]);
	let commandMenuOpen = $state(false);
	let loadingCommands = $state(false);
	let commandError = $state<string | null>(null);
	let highlightedCommand = $state(0);
	let commandHistory = $state<string[]>([]);
	let commandHistoryIndex = $state<number | null>(null);
	let commandHistoryDraft = $state("");

	const fallbackCommands: CommandInfo[] = [
		{ name: "help", description: "Show available commands" },
		{ name: "model", description: "View or set the model" },
		{ name: "providers", description: "List known providers and their auth status" },
		{ name: "status", description: "Show nexal system status" },
		{ name: "settings", description: "Manage provider config, auth, and tool keys" },
	];

	const commandQuery = $derived.by(() => {
		if (!value.startsWith("/") || value.includes("\n")) return null;
		const body = value.slice(1);
		if (body.includes(" ")) return null;
		return body.toLowerCase();
	});

	const visibleCommands = $derived.by(() => {
		const source = commands.length ? commands : fallbackCommands;
		const query = commandQuery;
		if (query === null) return [];
		return source
			.filter((command) => {
				const haystack = `${command.name} ${command.description}`.toLowerCase();
				return haystack.includes(query);
			})
			.slice(0, 8);
	});

	const showCommandMenu = $derived(
		commandMenuOpen && commandQuery !== null && visibleCommands.length > 0,
	);

	function autoResize() {
		if (!textareaEl) return;
		textareaEl.style.height = "auto";
		textareaEl.style.height = textareaEl.scrollHeight + "px";
	}

	async function refreshCommands() {
		if (commands.length || loadingCommands || chat.status !== "open") return;
		loadingCommands = true;
		commandError = null;
		try {
			commands = await chat.listCommands();
		} catch (err) {
			commandError = err instanceof Error ? err.message : "Unable to load commands";
		} finally {
			loadingCommands = false;
		}
	}

	function openCommandMenu() {
		if (commandQuery === null) {
			commandMenuOpen = false;
			return;
		}
		commandMenuOpen = true;
		highlightedCommand = 0;
		void refreshCommands();
	}

	function selectCommand(command: CommandInfo) {
		value = `/${command.name} `;
		commandMenuOpen = false;
		resetCommandHistoryCursor();
		queueMicrotask(() => {
			textareaEl?.focus();
			autoResize();
		});
	}

	function readCommandHistory(): string[] {
		if (typeof localStorage === "undefined") return [];
		try {
			const parsed = JSON.parse(localStorage.getItem(COMMAND_HISTORY_KEY) ?? "[]") as unknown;
			if (!Array.isArray(parsed)) return [];
			return parsed.filter((item): item is string => typeof item === "string" && item.startsWith("/"));
		} catch {
			return [];
		}
	}

	function writeCommandHistory(next: string[]) {
		if (typeof localStorage === "undefined") return;
		localStorage.setItem(COMMAND_HISTORY_KEY, JSON.stringify(next.slice(-MAX_COMMAND_HISTORY)));
	}

	function rememberCommand(text: string) {
		const command = text.trim();
		if (!command.startsWith("/")) return;
		const withoutDuplicateTail = commandHistory.filter((item) => item !== command);
		commandHistory = [...withoutDuplicateTail, command].slice(-MAX_COMMAND_HISTORY);
		writeCommandHistory(commandHistory);
		resetCommandHistoryCursor();
	}

	function resetCommandHistoryCursor() {
		commandHistoryIndex = null;
		commandHistoryDraft = "";
	}

	function canUseCommandHistory(e: KeyboardEvent): boolean {
		if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.isComposing) return false;
		if (value.includes("\n")) return false;
		if (value.trim() !== "" && !value.startsWith("/")) return false;
		return true;
	}

	function recallCommandHistory(direction: -1 | 1) {
		if (commandHistory.length === 0) return;
		if (commandHistoryIndex === null) {
			commandHistoryDraft = value;
			commandHistoryIndex = direction === -1 ? commandHistory.length - 1 : 0;
		} else {
			const nextIndex = commandHistoryIndex + direction;
			if (nextIndex < 0) {
				commandHistoryIndex = 0;
			} else if (nextIndex >= commandHistory.length) {
				commandHistoryIndex = null;
				value = commandHistoryDraft;
				queueMicrotask(autoResize);
				return;
			} else {
				commandHistoryIndex = nextIndex;
			}
		}
		value = commandHistory[commandHistoryIndex] ?? commandHistoryDraft;
		queueMicrotask(autoResize);
	}

	function onkeydown(e: KeyboardEvent) {
		if (showCommandMenu) {
			if (e.key === "ArrowDown") {
				e.preventDefault();
				highlightedCommand = Math.min(highlightedCommand + 1, visibleCommands.length - 1);
				return;
			}
			if (e.key === "ArrowUp") {
				e.preventDefault();
				highlightedCommand = Math.max(highlightedCommand - 1, 0);
				return;
			}
			if (e.key === "Tab" || e.key === "Enter") {
				const command = visibleCommands[highlightedCommand];
				if (command) {
					e.preventDefault();
					selectCommand(command);
					return;
				}
			}
			if (e.key === "Escape") {
				e.preventDefault();
				commandMenuOpen = false;
				return;
			}
		}

		if ((e.key === "ArrowUp" || e.key === "ArrowDown") && canUseCommandHistory(e)) {
			e.preventDefault();
			recallCommandHistory(e.key === "ArrowUp" ? -1 : 1);
			return;
		}

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
		rememberCommand(value);
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

	function readModelCache(): boolean {
		if (typeof localStorage === "undefined") return false;
		const raw = localStorage.getItem(MODEL_CACHE_KEY);
		if (!raw) return false;
		try {
			const cached = JSON.parse(raw) as ModelCache;
			providers = Array.isArray(cached.providers) ? cached.providers : [];
			activeModel = cached.active ?? null;
			hasModelCache = true;
			return true;
		} catch {
			localStorage.removeItem(MODEL_CACHE_KEY);
			return false;
		}
	}

	function writeModelCache(data: ProvidersData): void {
		if (typeof localStorage === "undefined") return;
		localStorage.setItem(
			MODEL_CACHE_KEY,
			JSON.stringify({ ...data, updatedAt: Date.now() } satisfies ModelCache),
		);
	}

	async function refreshModels(options: { silent?: boolean } = {}) {
		if (chat.status !== "open") {
			modelError = "Backend not connected";
			return;
		}
		if (!options.silent) loadingModels = true;
		modelError = null;
		try {
			const res = await chat.runCommandAwait("providers", []);
			if (res.error) throw new Error(res.error);
			const data = res.data as ProvidersData | null | undefined;
			const next = {
				providers: data?.providers ?? [],
				active: data?.active ?? null,
			} satisfies ProvidersData;
			providers = next.providers;
			activeModel = next.active;
			hasModelCache = true;
			writeModelCache(next);
		} catch (err) {
			modelError = err instanceof Error ? err.message : "Unable to load models";
			if (!options.silent && !hasModelCache) providers = [];
		} finally {
			if (!options.silent) loadingModels = false;
		}
	}

	async function toggleModelMenu() {
		modelMenuOpen = !modelMenuOpen;
		if (modelMenuOpen) commandMenuOpen = false;
		if (!modelMenuOpen) return;
		if (!hasModelCache) {
			const loaded = readModelCache();
			if (!loaded) await refreshModels();
			else void refreshModels({ silent: true });
			return;
		}
		void refreshModels({ silent: true });
	}

	onMount(() => {
		commandHistory = readCommandHistory();
		function closeModelMenu(event: PointerEvent) {
			if (!modelMenuOpen) return;
			if (modelMenuEl?.contains(event.target as Node)) return;
			modelMenuOpen = false;
		}

		document.addEventListener("pointerdown", closeModelMenu);
		return () => document.removeEventListener("pointerdown", closeModelMenu);
	});

	async function selectModel(provider: string, modelId: string) {
		selectingModel = `${provider}/${modelId}`;
		try {
			const res = await chat.runCommandAwait("model", [provider, modelId]);
			if (res.error) throw new Error(res.error);
			activeModel = { provider, modelId };
			writeModelCache({ providers, active: activeModel });
			hasModelCache = true;
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
		if (commandQuery !== null) openCommandMenu();
		else commandMenuOpen = false;
		highlightedCommand = Math.min(highlightedCommand, Math.max(visibleCommands.length - 1, 0));
	});
</script>

<div class="border-border bg-background relative rounded-2xl border shadow-sm">
	{#if showCommandMenu}
		<div class="border-border bg-popover text-popover-foreground absolute bottom-full left-0 right-0 z-20 mb-2 overflow-hidden rounded-md border shadow-lg">
			<div class="border-border flex items-center justify-between border-b px-3 py-2">
				<span class="text-sm font-medium">Commands</span>
				{#if loadingCommands}
					<span class="text-muted-foreground text-xs">Loading...</span>
				{:else if commandError}
					<span class="text-muted-foreground text-xs">Using local list</span>
				{/if}
			</div>
			<div class="max-h-72 overflow-y-auto py-1">
				{#each visibleCommands as command, index (command.name)}
					<button
						type="button"
						class={cn(
							"grid w-full grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-3 px-3 py-2 text-left transition-colors",
							index === highlightedCommand ? "bg-accent text-foreground" : "hover:bg-accent/70",
						)}
						onmousedown={(event) => event.preventDefault()}
						onclick={() => selectCommand(command)}
					>
						<span class="text-foreground mt-0.5 truncate font-mono text-sm">/{command.name}</span>
						<span class="text-muted-foreground min-w-0 text-sm leading-snug">{command.description}</span>
					</button>
				{/each}
			</div>
		</div>
	{/if}

	<textarea
		bind:this={textareaEl}
		bind:value
		oninput={() => {
			modelMenuOpen = false;
			resetCommandHistoryCursor();
			autoResize();
		}}
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

		<div class="relative" bind:this={modelMenuEl}>
			<button
				type="button"
				class="text-foreground/80 hover:bg-accent flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium"
				onclick={toggleModelMenu}
				title="Choose model"
				aria-expanded={modelMenuOpen}
			>
				<Icon icon={cpuLinear} class="size-3.5" />
				<span>{modelLabel}</span>
				<Icon icon={altArrowDownLinear} class="size-3.5 opacity-60" />
			</button>

			{#if modelMenuOpen}
				<div class="border-border bg-popover text-popover-foreground absolute bottom-8 left-0 z-20 flex h-96 w-72 flex-col overflow-hidden rounded-md border shadow-lg">
					<div class="border-border flex items-center gap-3 border-b px-3 py-2">
						<span class="text-sm font-medium">Models</span>
					</div>
					{#if loadingModels}
						<div class="text-muted-foreground flex min-h-0 flex-1 items-center justify-center gap-2 px-3 py-4 text-sm">
							<span class="border-muted-foreground/30 border-t-muted-foreground size-4 rounded-full border-2 animate-spin"></span>
							<span>Loading models...</span>
						</div>
					{:else if modelError}
						<div class="flex min-h-0 flex-1 items-center px-3 py-4 text-sm text-red-400">{modelError}</div>
					{:else if configuredProviders.length === 0}
						<div class="text-muted-foreground flex min-h-0 flex-1 items-center px-3 py-4 text-sm">No configured models.</div>
					{:else}
						<div class="min-h-0 flex-1 overflow-y-auto py-1">
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
