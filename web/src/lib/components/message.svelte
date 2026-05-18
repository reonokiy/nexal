<script lang="ts">
	import { renderMarkdown } from "$lib/markdown";
	import { cn } from "$lib/utils";
	import { settings } from "$lib/settings.svelte";
	import ChevronDown from "@lucide/svelte/icons/chevron-down";
	import ChevronRight from "@lucide/svelte/icons/chevron-right";
	import Terminal from "@lucide/svelte/icons/terminal";
	import Bot from "@lucide/svelte/icons/bot";

	export type Source = "coordinator" | "tool" | "worker";

	interface Props {
		role: "user" | "agent";
		source?: Source;
		text: string;
		ts: number;
		streaming?: boolean;
		toolName?: string;
		workerId?: string;
		workerStatus?: string;
	}
	let {
		role,
		source,
		text,
		ts,
		streaming = false,
		toolName,
		workerId,
		workerStatus,
	}: Props = $props();

	let expanded = $state(false);

	const html = $derived(role === "agent" && source !== "tool" ? renderMarkdown(text) : "");

	function fmt(t: number) {
		const d = new Date(t);
		return `${d.getHours().toString().padStart(2, "0")}:${d
			.getMinutes()
			.toString()
			.padStart(2, "0")}`;
	}

	function displayLabel(): string {
		if (role === "user") return "you";
		if (source === "coordinator") return "coordinator";
		if (source === "tool") return toolName ?? "tool";
		if (source === "worker") return "worker";
		return "nexal";
	}

	function labelColor(): string {
		if (role === "user") return "text-foreground";
		if (source === "coordinator") return "text-primary";
		if (source === "tool") return "text-amber-400";
		if (source === "worker") return "text-purple-400";
		return "text-primary";
	}

	function badgeClass(): string {
		if (source === "coordinator") return "bg-primary/10 text-primary";
		if (source === "tool") return "bg-amber-400/10 text-amber-400";
		if (source === "worker") return "bg-purple-400/10 text-purple-400";
		return "";
	}
</script>

<article
	class={cn(
		"group flex flex-col gap-1",
		settings.compact ? "py-2" : "py-3",
		source === "tool" && "pl-4 border-l-2 border-amber-400/20",
		source === "worker" && "pl-4 border-l-2 border-purple-400/20",
	)}
>
	<div class="text-muted-foreground flex items-baseline gap-2 text-xs">
		<span class={cn("font-semibold flex items-center gap-1", labelColor())}>
			{#if source === "tool"}
				<Terminal class="size-3" />
			{:else if source === "worker"}
				<Bot class="size-3" />
			{/if}
			{displayLabel()}
		</span>
		{#if source && source !== "coordinator"}
			<span class={cn("text-[10px] font-medium rounded px-1 py-0.5", badgeClass())}>
				{source}
			</span>
		{/if}
		{#if settings.showTimestamps}
			<time class="opacity-60">{fmt(ts)}</time>
		{/if}
		{#if streaming}
			<span class="text-primary inline-flex items-center gap-1 text-[10px]">
				<span class="bg-primary size-1.5 animate-pulse rounded-full"></span>
				streaming
			</span>
		{/if}
	</div>

	{#if source === "tool"}
		<div class="mt-1">
			<button
				type="button"
				class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
				onclick={() => (expanded = !expanded)}
			>
				{#if expanded}
					<ChevronDown class="size-3" />
				{:else}
					<ChevronRight class="size-3" />
				{/if}
				<span>{expanded ? "Hide output" : "Show output"}</span>
			</button>
			{#if expanded}
				<div class="mt-1.5 bg-muted/50 rounded-md p-3 font-mono text-xs whitespace-pre-wrap break-words text-muted-foreground">
					{text}
				</div>
			{/if}
		</div>
	{:else if source === "worker"}
		<div class="mt-1 bg-purple-400/5 rounded-md p-3 text-sm">
			<div class="flex items-center gap-2 text-xs text-muted-foreground mb-1">
				<span class="font-mono">{workerId ?? "unknown"}</span>
				<span class="inline-flex items-center gap-1">
					<span class="size-1.5 rounded-full bg-purple-400"></span>
					{workerStatus ?? "running"}
				</span>
			</div>
			<div class="text-muted-foreground whitespace-pre-wrap break-words">
				{text}
			</div>
		</div>
	{:else if role === "agent"}
		<div class="prose prose-sm md-body text-foreground max-w-none">
			{@html html}{#if streaming}<span class="md-cursor"></span>{/if}
		</div>
	{:else}
		<div class="text-foreground/90 text-sm leading-relaxed whitespace-pre-wrap break-words">
			{text}
		</div>
	{/if}
</article>
