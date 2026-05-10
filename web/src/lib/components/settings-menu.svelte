<script lang="ts">
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import Settings from "@lucide/svelte/icons/settings";
	import Check from "@lucide/svelte/icons/check";
	import { cn } from "$lib/utils";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	interface Preset {
		provider: string;
		modelId: string;
		label: string;
	}

	const PRESETS: Preset[] = [
		{ provider: "anthropic", modelId: "claude-opus-4-7", label: "Claude Opus 4.7" },
		{ provider: "anthropic", modelId: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
		{ provider: "anthropic", modelId: "claude-haiku-4-5", label: "Claude Haiku 4.5" },
		{ provider: "openai", modelId: "gpt-4o", label: "GPT-4o" },
		{ provider: "openai", modelId: "gpt-4o-mini", label: "GPT-4o mini" },
		{ provider: "openrouter", modelId: "openai/gpt-4o", label: "OpenRouter · GPT-4o" },
		{ provider: "openrouter", modelId: "anthropic/claude-3.5-sonnet", label: "OpenRouter · Sonnet 3.5" },
		{ provider: "google", modelId: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
		{ provider: "mistral", modelId: "mistral-large-latest", label: "Mistral Large" },
	];

	const PROVIDERS = ["anthropic", "openai", "openrouter", "google", "mistral"];

	let open = $state(false);
	let provider = $state("anthropic");
	let modelId = $state("claude-sonnet-4-6");
	let menuEl: HTMLDivElement | undefined = $state();

	function toggle() {
		open = !open;
		if (open) chat.runCommand("model"); // fetch current — prints to chat
	}

	function close() {
		open = false;
	}

	function applyPreset(p: Preset) {
		provider = p.provider;
		modelId = p.modelId;
	}

	function save() {
		const p = provider.trim();
		const m = modelId.trim();
		if (!p || !m) return;
		chat.runCommand("model", [p, m]);
		open = false;
	}

	function onDocClick(ev: MouseEvent) {
		if (!open || !menuEl) return;
		if (!menuEl.contains(ev.target as Node)) close();
	}

	$effect(() => {
		if (open) {
			document.addEventListener("mousedown", onDocClick);
			return () => document.removeEventListener("mousedown", onDocClick);
		}
	});

	const matched = $derived(
		PRESETS.find((p) => p.provider === provider && p.modelId === modelId),
	);
</script>

<div class="relative" bind:this={menuEl}>
	<Button variant="outline" size="sm" onclick={toggle}>
		<Settings />
		<span class="font-mono text-xs">{provider} / {modelId}</span>
	</Button>

	{#if open}
		<div
			class="bg-popover text-popover-foreground border-border absolute right-0 top-[calc(100%+0.5rem)] z-20 w-96 rounded-md border p-4 shadow-md"
			role="dialog"
		>
			<div class="mb-3 flex items-baseline justify-between">
				<h3 class="text-sm font-semibold">Model provider</h3>
				<span class="text-muted-foreground text-[10px]">Restart nexal to apply</span>
			</div>

			<div class="grid grid-cols-[5rem_1fr] items-center gap-2">
				<label for="settings-provider" class="text-xs">Provider</label>
				<select
					id="settings-provider"
					bind:value={provider}
					class="border-input bg-background h-9 rounded-md border px-2 text-sm"
				>
					{#each PROVIDERS as p (p)}
						<option value={p}>{p}</option>
					{/each}
				</select>

				<label for="settings-model" class="text-xs">Model id</label>
				<Input
					id="settings-model"
					class="font-mono text-xs"
					bind:value={modelId}
				/>
			</div>

			<div class="mt-3">
				<div class="text-muted-foreground mb-1.5 text-[10px] uppercase tracking-wide">
					Presets
				</div>
				<ul class="border-border max-h-40 overflow-y-auto rounded-md border">
					{#each PRESETS as p (p.label)}
						{@const active = matched === p}
						<li>
							<button
								type="button"
								onclick={() => applyPreset(p)}
								class={cn(
									"hover:bg-accent flex w-full items-center justify-between px-3 py-1.5 text-left text-xs",
									active && "bg-accent",
								)}
							>
								<span class="flex flex-col">
									<span class="font-medium">{p.label}</span>
									<span class="text-muted-foreground font-mono text-[10px]">
										{p.provider} / {p.modelId}
									</span>
								</span>
								{#if active}
									<Check class="size-3.5" />
								{/if}
							</button>
						</li>
					{/each}
				</ul>
			</div>

			<div class="mt-4 flex items-center justify-between gap-2">
				<span class="text-muted-foreground text-[11px]">
					OAuth login (e.g. Anthropic) only via terminal: <code>nexal -i</code> →
					<code>/login</code>
				</span>
				<div class="flex gap-2">
					<Button variant="ghost" size="sm" onclick={close}>cancel</Button>
					<Button size="sm" onclick={save}>save</Button>
				</div>
			</div>
		</div>
	{/if}
</div>
