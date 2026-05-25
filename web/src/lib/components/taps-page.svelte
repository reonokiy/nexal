<script lang="ts">
	import Icon from "@iconify/svelte";
	import {
		billListLinear,
		clockCircleLinear,
		codeSquareLinear,
		hashtagLinear,
		infoCircleLinear,
		recordCircleLinear,
		starsMinimalisticLinear,
		userRoundedLinear,
	} from "$lib/icons/solar";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import { renderMarkdown } from "$lib/markdown";
	import type { Chat } from "$lib/client.svelte";

	interface TapeInfo {
		id: string;
		entries: number;
		anchors: number;
		lastAnchor: string | null;
		entriesSinceLastAnchor: number;
		lastTokenUsage: number | null;
	}

	interface TapeEntry {
		id: number;
		kind: string;
		payload: Record<string, unknown>;
		meta: Record<string, unknown>;
		date: string;
	}

	let { chat }: { chat: Chat } = $props();

	let tape = $state<TapeInfo | null>(null);
	let entries = $state<TapeEntry[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	const tapeId = $derived.by(() => {
		const match = router.current.match(/^tap(?:e)?s\/(.+)$/);
		return match?.[1] ? decodeURIComponent(match[1]) : null;
	});

	async function loadTape(id: string) {
		if (chat.status !== "open") {
			error = "Backend not connected";
			return;
		}

		try {
			loading = true;
			error = null;
			const res = await chat.runCommandAwait("tape", [id], 10_000);
			if (res.error) throw new Error(res.error);
			const data = res.data as { tape?: TapeInfo; entries?: TapeEntry[] } | null | undefined;
			tape = data?.tape ?? null;
			entries = data?.entries ?? [];
		} catch (e) {
			tape = null;
			entries = [];
			error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			loading = false;
		}
	}

	let loadedTapeId: string | null = null;
	$effect(() => {
		if (!tapeId || tapeId === loadedTapeId) return;
		loadedTapeId = tapeId;
		void loadTape(tapeId);
	});

	function fmt(value: string): string {
		const date = new Date(value);
		if (Number.isNaN(date.getTime())) return value;
		return new Intl.DateTimeFormat(undefined, {
			month: "short",
			day: "numeric",
			hour: "2-digit",
			minute: "2-digit",
		}).format(date);
	}

	function entryIcon(entry: TapeEntry) {
		const role = String(entry.payload.role ?? "");
		if (role === "user") return userRoundedLinear;
		if (role === "assistant") return starsMinimalisticLinear;
		if (entry.kind === "anchor") return hashtagLinear;
		return codeSquareLinear;
	}

	function entryLabel(entry: TapeEntry): string {
		const role = String(entry.payload.role ?? "");
		if (role === "user") return "User";
		if (role === "assistant") return "Assistant";
		if (entry.kind === "tool_result") return String(entry.payload.toolName ?? "Tool result");
		if (entry.kind === "anchor") return `Anchor ${String(entry.payload.name ?? "")}`.trim();
		return entry.kind;
	}

	function entryClass(entry: TapeEntry): string {
		const role = String(entry.payload.role ?? "");
		if (role === "user") return "bg-sky-500/10 text-sky-700 dark:text-sky-300";
		if (role === "assistant") return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
		if (entry.kind === "anchor") return "bg-violet-500/10 text-violet-700 dark:text-violet-300";
		if (entry.kind === "tool_result") return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
		return "bg-zinc-500/10 text-zinc-700 dark:text-zinc-300";
	}

	function textFromContent(content: unknown): string {
		if (typeof content === "string") return content;
		if (Array.isArray(content)) {
			return content
				.map((item) => {
					if (typeof item === "string") return item;
					if (!item || typeof item !== "object") return "";
					const record = item as Record<string, unknown>;
					if (typeof record.text === "string") return record.text;
					if (typeof record.content === "string") return record.content;
					if (typeof record.type === "string") return `[${record.type}]`;
					return "";
				})
				.filter(Boolean)
				.join("\n");
		}
		if (content == null) return "";
		return JSON.stringify(content, null, 2);
	}

	function bodyFor(entry: TapeEntry): string {
		if ("content" in entry.payload) return textFromContent(entry.payload.content);
		if (entry.kind === "anchor") {
			const state = entry.payload.state;
			return state ? JSON.stringify(state, null, 2) : "Checkpoint";
		}
		return JSON.stringify(entry.payload, null, 2);
	}

	function isMarkdown(entry: TapeEntry): boolean {
		return String(entry.payload.role ?? "") === "assistant";
	}
</script>

<section class="flex min-h-full w-full flex-col px-6 pb-8">
	<div class="mx-auto flex w-full max-w-5xl flex-col gap-5">
		<div class="flex flex-wrap items-end justify-between gap-4 pt-2">
			<div>
				<div class="text-muted-foreground mb-1 flex items-center gap-2 text-xs font-medium uppercase">
					<Icon icon={billListLinear} class="size-3.5" />
					<span>Tapes</span>
				</div>
				<h1 class="text-2xl font-semibold tracking-tight">
					{tape ? tape.id : "Select a tape"}
				</h1>
			</div>
			{#if tape}
				<div class="text-muted-foreground flex flex-wrap items-center gap-3 text-sm">
					<span>{tape.entries} entries</span>
					<span>{tape.anchors} anchors</span>
					{#if tape.lastAnchor}
						<span>last: {tape.lastAnchor}</span>
					{/if}
				</div>
			{/if}
		</div>

		{#if !tapeId}
			<div class="border-border flex min-h-80 flex-col items-center justify-center rounded-md border border-dashed text-center">
				<Icon icon={infoCircleLinear} class="text-muted-foreground mb-3 size-8" />
				<p class="text-sm font-medium">Choose a tape</p>
				<p class="text-muted-foreground mt-1 text-sm">Pick one from the Tapes section in the sidebar.</p>
			</div>
		{:else if loading}
			<div class="text-muted-foreground flex min-h-80 items-center justify-center gap-2 text-sm">
				<Icon icon={recordCircleLinear} class="size-4 animate-spin" />
				<span>Loading tape…</span>
			</div>
		{:else if error}
			<div class="flex min-h-80 items-center justify-center text-sm text-red-400">{error}</div>
		{:else if entries.length === 0}
			<div class="border-border flex min-h-80 flex-col items-center justify-center rounded-md border border-dashed text-center">
				<Icon icon={billListLinear} class="text-muted-foreground mb-3 size-8" />
				<p class="text-sm font-medium">This tape is empty</p>
			</div>
		{:else}
			<div class="border-border bg-background overflow-hidden rounded-md border">
				{#each entries as entry (entry.id)}
					<article class="border-border grid gap-3 border-b px-4 py-4 last:border-b-0 md:grid-cols-[9rem_minmax(0,1fr)]">
						<div class="flex items-start gap-2">
							<span class={cn("mt-0.5 flex size-7 shrink-0 items-center justify-center rounded", entryClass(entry))}>
								<Icon icon={entryIcon(entry)} class="size-3.5" />
							</span>
							<div class="min-w-0">
								<div class="text-sm font-medium">#{entry.id}</div>
								<div class="text-muted-foreground mt-0.5 flex items-center gap-1 text-xs">
									<Icon icon={clockCircleLinear} class="size-3" />
									<span>{fmt(entry.date)}</span>
								</div>
							</div>
						</div>
						<div class="min-w-0">
							<div class="mb-2 flex flex-wrap items-center gap-2">
								<span class={cn("inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium", entryClass(entry))}>
									{entryLabel(entry)}
								</span>
								<span class="text-muted-foreground rounded bg-muted px-1.5 py-0.5 text-xs">{entry.kind}</span>
							</div>
							{#if isMarkdown(entry)}
								<div class="md-body text-foreground max-w-none text-sm">
									{@html renderMarkdown(bodyFor(entry))}
								</div>
							{:else}
								<pre class="text-foreground/90 whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{bodyFor(entry)}</pre>
							{/if}
						</div>
					</article>
				{/each}
			</div>
		{/if}
	</div>
</section>
