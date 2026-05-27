<script lang="ts">
	import Icon from "@iconify/svelte";
	import {
		billListLinear,
		infoCircleLinear,
		recordCircleLinear,
	} from "$lib/icons/solar";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import { renderMarkdown } from "$lib/markdown";
	import type { Chat } from "$lib/client.svelte";
	import { onMount } from "svelte";
	import { VList } from "virtua/svelte";
	import type { VListHandle } from "virtua/svelte";

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

	interface TapeEntriesPage {
		tape?: TapeInfo;
		entries?: TapeEntry[];
		offset?: number;
		limit?: number;
		total?: number;
		hasMore?: boolean;
	}

	type ContentBlock =
		| { type: "text" | "thinking"; text: string }
		| { type: "image"; src: string; mimeType: string }
		| { type: "toolCall"; id: string; name: string; args: unknown };

	type TapeHeaderInfo = {
		id: string;
		entries: number;
		anchors: number;
		lastAnchor: string | null;
	} | null;

	let {
		chat,
		onHeaderChange,
		onTapesCountChange,
	}: {
		chat: Chat;
		onHeaderChange?: (info: TapeHeaderInfo) => void;
		onTapesCountChange?: (count: number | null) => void;
	} = $props();

	let tape = $state<TapeInfo | null>(null);
	let tapes = $state<TapeInfo[]>([]);
	let entries = $state<TapeEntry[]>([]);
	let loading = $state(false);
	let loadingMore = $state(false);
	let loadingTapes = $state(false);
	let error = $state<string | null>(null);
	let tapesError = $state<string | null>(null);
	let hasMoreEntries = $state(false);
	let nextEntryOffset = $state(0);
	let tapeList: VListHandle | null = $state(null);

	const TAPE_PAGE_SIZE = 100;

	const tapeId = $derived.by(() => {
		const match = router.current.match(/^tap(?:e)?s\/(.+)$/);
		return match?.[1] ? decodeURIComponent(match[1]) : null;
	});

	async function loadTape(id: string) {
		if (chat.status !== "open") {
			loading = true;
			error = null;
			return;
		}

		try {
			loading = true;
			loadingMore = false;
			error = null;
			hasMoreEntries = false;
			nextEntryOffset = 0;
			entries = [];
			const [info, page] = await Promise.all([
				loadTapeInfo(id),
				loadTapeEntriesPage(id, 0),
			]);
			tape = page.tape ?? info;
			entries = page.entries ?? [];
			nextEntryOffset = entries.length;
			hasMoreEntries = page.hasMore ?? entries.length < (page.total ?? tape.entries);
			setTimeout(checkNeedMoreEntries, 0);
		} catch (e) {
			tape = null;
			entries = [];
			hasMoreEntries = false;
			nextEntryOffset = 0;
			error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			loading = false;
		}
	}

	async function loadTapeInfo(id: string): Promise<TapeInfo> {
		const res = await chat.runCommandAwait("tape", [id], 30_000);
		if (res.error) throw new Error(res.error);
		const data = res.data as { tape?: TapeInfo } | null | undefined;
		if (!data?.tape) throw new Error("Tape metadata missing");
		return data.tape;
	}

	async function loadTapeEntriesPage(id: string, offset: number): Promise<TapeEntriesPage> {
		const res = await chat.runCommandAwait(
			"tape_entries",
			[id, String(offset), String(TAPE_PAGE_SIZE)],
			30_000,
		);
		if (res.error) throw new Error(res.error);
		return (res.data as TapeEntriesPage | null | undefined) ?? {};
	}

	async function loadMoreEntries() {
		if (!tapeId || loading || loadingMore || !hasMoreEntries || chat.status !== "open") return;
		try {
			loadingMore = true;
			const page = await loadTapeEntriesPage(tapeId, nextEntryOffset);
			const nextEntries = page.entries ?? [];
			if (page.tape) tape = page.tape;
			if (nextEntries.length > 0) {
				entries = [...entries, ...nextEntries];
			}
			nextEntryOffset += nextEntries.length;
			hasMoreEntries = page.hasMore ?? nextEntryOffset < (page.total ?? tape?.entries ?? nextEntryOffset);
			setTimeout(checkNeedMoreEntries, 0);
		} catch (e) {
			error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			loadingMore = false;
		}
	}

	function checkNeedMoreEntries() {
		if (!tapeList || !hasMoreEntries || loading || loadingMore) return;
		const remaining =
			tapeList.getScrollSize() - tapeList.getScrollOffset() - tapeList.getViewportSize();
		if (remaining < 1200) void loadMoreEntries();
	}

	async function loadTapes(silent = false) {
		if (chat.status !== "open") {
			if (!silent) loadingTapes = false;
			tapesError = "Backend not connected";
			return;
		}

		try {
			if (!silent) loadingTapes = true;
			tapesError = null;
			const res = await chat.runCommandAwait("tapes", [], 30_000);
			if (res.error) throw new Error(res.error);
			const data = res.data as { tapes?: TapeInfo[] } | null | undefined;
			tapes = data?.tapes ?? [];
		} catch (e) {
			if (!silent) {
				tapes = [];
				tapesError = e instanceof Error ? e.message : "fetch failed";
			}
		} finally {
			if (!silent) loadingTapes = false;
		}
	}

	let loadedTapeId: string | null = null;
	let loadedTapesForRoute: string | null = null;
	$effect(() => {
		if (!tapeId) {
			tape = null;
			entries = [];
			hasMoreEntries = false;
			nextEntryOffset = 0;
			loadedTapeId = null;
			if (chat.status !== "open") {
				loadingTapes = false;
				tapesError = "Backend not connected";
				return;
			}
			const routeKey = `${chat.status}:${router.current}`;
			if (loadedTapesForRoute === routeKey) return;
			loadedTapesForRoute = routeKey;
			void loadTapes();
			return;
		}
		if (chat.status !== "open") {
			loading = true;
			error = null;
			loadingMore = false;
			return;
		}
		if (tapeId === loadedTapeId) return;
		loadedTapeId = tapeId;
		void loadTape(tapeId);
	});

	onMount(() => {
		const interval = setInterval(() => {
			if (!tapeId && chat.status === "open") void loadTapes(true);
		}, 10_000);
		return () => clearInterval(interval);
	});

	$effect(() => {
		onHeaderChange?.(
			tape
				? {
						id: tape.id,
						entries: tape.entries,
						anchors: tape.anchors,
						lastAnchor: tape.lastAnchor,
					}
				: null,
		);
	});

	$effect(() => {
		onTapesCountChange?.(!tapeId && !loadingTapes && !tapesError ? tapes.length : null);
	});

	function fmt(value: string): string {
		const date = new Date(value);
		if (Number.isNaN(date.getTime())) return value;
		return new Intl.DateTimeFormat(undefined, {
			year: "numeric",
			month: "2-digit",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
			second: "2-digit",
			hour12: false,
		}).format(date);
	}

	function entryLabel(entry: TapeEntry): string {
		const role = String(entry.payload.role ?? "");
		if (role === "user") return "User";
		if (role === "assistant") return "Assistant";
		if (entry.kind === "tool_call") return String(entry.payload.toolName ?? entry.payload.name ?? "Tool call");
		if (entry.kind === "tool_result") return String(entry.payload.toolName ?? "Tool result");
		if (entry.kind === "anchor") return "Anchor";
		return entry.kind;
	}

	function shortTapeId(id: string): string {
		return id.length > 36 ? `${id.slice(0, 36)}...` : id;
	}

	function tapeSubtitle(item: TapeInfo): string {
		const parts = [`${item.entries} entries`, `${item.anchors} anchors`];
		if (item.entriesSinceLastAnchor > 0) {
			parts.push(`${item.entriesSinceLastAnchor} since last anchor`);
		}
		return parts.join(" · ");
	}

	function entryKindLabel(entry: TapeEntry): string {
		if (entry.kind === "anchor") return String(entry.payload.name ?? "anchor");
		return entry.kind;
	}

	function entryClass(entry: TapeEntry): string {
		const role = String(entry.payload.role ?? "");
		if (role === "user") return "bg-sky-500/10 text-sky-700 dark:text-sky-300";
		if (role === "assistant") return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
		if (entry.kind === "anchor") return "bg-violet-500/10 text-violet-700 dark:text-violet-300";
		if (entry.kind === "tool_call") return "bg-orange-500/10 text-orange-700 dark:text-orange-300";
		if (entry.kind === "tool_result") return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
		return "bg-zinc-500/10 text-zinc-700 dark:text-zinc-300";
	}

	function formatToolArgs(value: unknown): string {
		if (typeof value === "string") return value;
		if (value == null) return "{}";
		try {
			return JSON.stringify(value, null, 2);
		} catch {
			return String(value);
		}
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
		if (entry.kind === "tool_call") {
			const name = String(entry.payload.toolName ?? entry.payload.name ?? "tool");
			const id = String(entry.payload.toolCallId ?? entry.payload.id ?? "");
			const args = entry.payload.arguments ?? entry.payload.args ?? entry.payload.input ?? {};
			return `${name}${id ? ` (${id})` : ""}\n${formatToolArgs(args)}`;
		}
		if (entry.kind === "anchor") {
			const state = entry.payload.state;
			return state ? JSON.stringify(state, null, 2) : "Checkpoint";
		}
		return JSON.stringify(entry.payload, null, 2);
	}

	function isMarkdown(entry: TapeEntry): boolean {
		return String(entry.payload.role ?? "") === "assistant";
	}

	function imageSrc(record: Record<string, unknown>): string | null {
		const mimeType = typeof record.mimeType === "string" ? record.mimeType : "image/png";
		const url = record.url;
		if (typeof url === "string" && url) return url;
		const data = record.data;
		if (typeof data === "string") {
			return data.startsWith("data:") ? data : `data:${mimeType};base64,${data}`;
		}
		return null;
	}

	function contentBlocks(entry: TapeEntry): ContentBlock[] {
		if (!("content" in entry.payload)) return [{ type: "text", text: bodyFor(entry) }];
		const content = entry.payload.content;
		if (typeof content === "string") return [{ type: "text", text: content }];
		if (!Array.isArray(content)) return [{ type: "text", text: bodyFor(entry) }];

		return content
			.map((item): ContentBlock | null => {
				if (typeof item === "string") return { type: "text", text: item };
				if (!item || typeof item !== "object") return null;
				const record = item as Record<string, unknown>;
				if (record.type === "image") {
					const src = imageSrc(record);
					if (!src) return null;
					return {
						type: "image",
						src,
						mimeType: typeof record.mimeType === "string" ? record.mimeType : "image/png",
					};
				}
				if (record.type === "toolCall" || record.type === "tool_call") {
					return {
						type: "toolCall",
						id: String(record.id ?? record.toolCallId ?? ""),
						name: String(record.name ?? record.toolName ?? "tool"),
						args: record.arguments ?? record.args ?? record.input ?? {},
					};
				}
				const text =
					typeof record.thinking === "string"
						? record.thinking
						: typeof record.text === "string"
							? record.text
							: typeof record.content === "string"
								? record.content
								: "";
				if (!text) return null;
				return {
					type: record.type === "thinking" ? "thinking" : "text",
					text,
				};
			})
			.filter((block): block is ContentBlock => block !== null);
	}
	</script>

<section class="flex h-full min-h-0 w-full flex-col px-6 pb-8">
	<div class="mx-auto flex h-full min-h-0 w-full max-w-5xl flex-col gap-5">
		{#if !tapeId}
			{#if loadingTapes}
				<div class="text-muted-foreground flex min-h-80 items-center justify-center gap-2 text-sm">
					<Icon icon={recordCircleLinear} class="size-4 animate-spin" />
					<span>Loading tapes…</span>
				</div>
			{:else if tapesError}
				<div class="flex min-h-80 items-center justify-center text-sm text-red-400">{tapesError}</div>
			{:else if tapes.length === 0}
				<div class="border-border flex min-h-80 flex-col items-center justify-center rounded-md border border-dashed text-center">
					<Icon icon={infoCircleLinear} class="text-muted-foreground mb-3 size-8" />
					<p class="text-sm font-medium">No tapes yet</p>
				</div>
			{:else}
				<div class="flex min-h-0 flex-col gap-3">
					<div class="border-border overflow-hidden rounded-md border">
						{#each tapes as item (item.id)}
							<button
								type="button"
								class="border-border hover:bg-accent flex w-full items-center gap-3 border-b px-4 py-3 text-left transition-colors duration-150 last:border-b-0"
								onclick={() => router.go(`tapes/${encodeURIComponent(item.id)}`)}
								title={item.id}
							>
								<div class="bg-muted text-muted-foreground flex size-9 shrink-0 items-center justify-center rounded-md">
									<Icon icon={billListLinear} class="size-4" />
								</div>
								<div class="min-w-0 flex-1">
									<div class="truncate text-sm font-medium">{shortTapeId(item.id)}</div>
									<div class="text-muted-foreground mt-0.5 truncate text-xs">{tapeSubtitle(item)}</div>
								</div>
								{#if item.lastTokenUsage != null}
									<div class="text-muted-foreground shrink-0 text-xs">{item.lastTokenUsage} tokens</div>
								{/if}
							</button>
						{/each}
					</div>
				</div>
			{/if}
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
			<div class="relative min-h-0 flex-1">
				<VList
					bind:this={tapeList}
					data={entries}
					getKey={(entry: TapeEntry) => entry.id}
					onscroll={checkNeedMoreEntries}
					onscrollend={checkNeedMoreEntries}
					class="border-border bg-background rounded-md border"
					style="height: 100%; width: 100%;"
				>
					{#snippet children(entry: TapeEntry)}
						<article class="border-border border-b px-4 py-4">
							<div class="min-w-0">
								<div class="mb-2 flex min-w-0 items-center gap-2">
									<div class="flex min-w-0 flex-wrap items-center gap-2">
										<span class={cn("inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium", entryClass(entry))}>
											{entryLabel(entry)}
										</span>
										<span class="text-muted-foreground rounded bg-muted px-1.5 py-0.5 text-xs">{entryKindLabel(entry)}</span>
									</div>
									<div class="text-muted-foreground ml-auto flex shrink-0 items-center gap-2 text-xs">
										<span>{fmt(entry.date)}</span>
										<span class="font-medium text-foreground">#{entry.id}</span>
									</div>
								</div>
								<div class="flex flex-col gap-2">
									{#each contentBlocks(entry) as block}
										{#if block.type === "thinking"}
											<div class="border-border bg-muted/35 text-muted-foreground rounded-md border px-3 py-2 text-sm">
												<div class="mb-1 text-[10px] font-medium uppercase tracking-wider">thinking</div>
												<pre class="whitespace-pre-wrap break-words font-sans leading-relaxed">{block.text}</pre>
											</div>
										{:else if block.type === "image"}
											<figure class="border-border bg-muted/20 overflow-hidden rounded-md border">
												<img
													src={block.src}
													alt={`Tape image (${block.mimeType})`}
													class="max-h-[520px] w-auto max-w-full object-contain"
													loading="lazy"
													decoding="async"
												/>
												<figcaption class="text-muted-foreground border-border border-t px-3 py-1.5 text-xs">
													{block.mimeType}
												</figcaption>
											</figure>
										{:else if block.type === "toolCall"}
											<div class="border-border rounded-md border bg-orange-500/[0.03] px-3 py-2 text-sm">
												<div class="mb-2 flex min-w-0 items-center gap-2">
													<span class="text-orange-700 dark:text-orange-300 text-[10px] font-medium uppercase tracking-wider">tool call</span>
													<span class="truncate font-medium">{block.name}</span>
													{#if block.id}
														<span class="text-muted-foreground ml-auto shrink-0 truncate text-xs">{block.id}</span>
													{/if}
												</div>
												<pre class="text-foreground/90 whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{formatToolArgs(block.args)}</pre>
											</div>
										{:else if isMarkdown(entry)}
											<div class="md-body text-foreground max-w-none text-sm">
												{@html renderMarkdown(block.text)}
											</div>
										{:else}
											<pre class="text-foreground/90 whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{block.text}</pre>
										{/if}
									{/each}
								</div>
							</div>
						</article>
					{/snippet}
				</VList>
				{#if loadingMore}
					<div class="bg-background/90 border-border text-muted-foreground absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-md border px-3 py-1.5 text-xs shadow-sm">
						<Icon icon={recordCircleLinear} class="size-3.5 animate-spin" />
						<span>Loading more…</span>
					</div>
				{/if}
			</div>
		{/if}
	</div>
</section>
