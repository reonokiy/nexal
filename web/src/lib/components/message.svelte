<script lang="ts">
	import { renderMarkdown } from "$lib/markdown";
	import { cn } from "$lib/utils";
	import { settings } from "$lib/settings.svelte";
	import Icon from "@iconify/svelte";
	import {
		altArrowDownLinear,
		altArrowRightLinear,
		codeSquareLinear,
	} from "$lib/icons/solar";

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
		workerPath?: string;
		showHeader?: boolean;
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
		workerPath,
		showHeader = true,
	}: Props = $props();

	let toolExpanded = $state(false);
	let messageExpanded = $state(false);

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
		if (source === "worker") return workerPath ?? `coordinator > ${workerId ?? "worker"}`;
		return "nexal";
	}

	function labelColor(): string {
		if (role === "user") return "text-foreground";
		if (source === "coordinator") return "text-primary";
		if (source === "tool") return "text-amber-400";
		if (source === "worker") return "text-foreground";
		return "text-primary";
	}

	function badgeClass(): string {
		if (source === "coordinator") return "bg-primary/10 text-primary";
		if (source === "tool") return "bg-amber-400/10 text-amber-400";
		return "";
	}
</script>

<article
	class={cn(
		"group flex flex-col gap-1",
		showHeader ? (settings.compact ? "py-2" : "py-3") : "py-1",
		source === "tool" && "pl-4 border-l-2 border-amber-400/20",
	)}
>
	{#if showHeader}
		<div class="text-muted-foreground flex items-baseline gap-2 text-xs">
			<span class={cn("font-semibold flex items-center gap-1", labelColor())}>
				{#if source === "tool"}
					<Icon icon={codeSquareLinear} class="size-3" />
				{/if}
				{displayLabel()}
			</span>
			{#if source && source !== "coordinator" && source !== "worker"}
				<span class={cn("text-[10px] font-medium rounded px-1 py-0.5", badgeClass())}>
					{source}
				</span>
			{/if}
			{#if source === "worker"}
				<span class="text-muted-foreground/80 text-[10px]">
					{workerStatus ?? "running"}
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
	{/if}

	{#if source === "tool"}
		<div class="mt-1">
			<button
				type="button"
				class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
				onclick={() => (toolExpanded = !toolExpanded)}
			>
				{#if toolExpanded}
					<Icon icon={altArrowDownLinear} class="size-3" />
				{:else}
					<Icon icon={altArrowRightLinear} class="size-3" />
				{/if}
				<span>{toolExpanded ? "Hide output" : "Show output"}</span>
			</button>
			{#if toolExpanded}
				<div class="mt-1.5 bg-muted/50 rounded-md p-3 font-mono text-xs whitespace-pre-wrap break-words text-muted-foreground">
					{text}
				</div>
			{/if}
		</div>
	{:else if source === "worker"}
		<div class="mt-1 text-sm">
			<button
				type="button"
				class="text-muted-foreground hover:text-foreground flex items-center gap-1 text-xs transition-colors"
				onclick={() => (messageExpanded = !messageExpanded)}
			>
				{#if messageExpanded}
					<Icon icon={altArrowDownLinear} class="size-3" />
					<span>Hide message</span>
				{:else}
					<Icon icon={altArrowRightLinear} class="size-3" />
					<span>Show message</span>
				{/if}
			</button>
			{#if messageExpanded}
				<div class="text-foreground/85 mt-1.5 whitespace-pre-wrap break-words text-sm leading-relaxed">
					{text}
				</div>
			{/if}
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
