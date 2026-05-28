<script lang="ts">
	import Icon from "@iconify/svelte";
	import {
		altArrowRightLinear,
		billListLinear,
		infoCircleLinear,
		recordCircleLinear,
		trashBinTrashLinear,
	} from "$lib/icons/solar";
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import Markdown from "$lib/components/markdown.svelte";
	import type { Chat } from "$lib/client.svelte";
	import {
		getCachedLatestTapeEntries,
		getCachedTapeEntriesPage,
		getCachedTapeInfo,
		getCachedTapes,
		deleteCachedTape,
		putCachedTapeEntries,
		putCachedTapeInfo,
		putCachedTapes,
	} from "$lib/tape-cache";
	import { onMount, tick } from "svelte";
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
		| { type: "toolCall"; id: string; name: string; args: unknown }
		| {
				type: "tapeRef";
				tapeId: string;
				relation?: string;
				name?: string;
				kind?: string;
				lifetime?: string;
				agentId?: string;
			};

	type TapeHeaderInfo = {
		id: string;
		entries: number;
		anchors: number;
		lastAnchor: string | null;
	} | null;

	type ToolArgRow = {
		key: string;
		value: string;
		kind: "string" | "number" | "boolean" | "null" | "object" | "array";
	};

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
	let syncing = $state(false);
	let deletingTapeId = $state<string | null>(null);
	let pendingDeleteTape = $state<TapeInfo | null>(null);
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
		const generation = ++loadTapeGeneration;
		loading = true;
		loadingMore = false;
		syncing = false;
		error = null;
		hasMoreEntries = false;
		nextEntryOffset = 0;
		entries = [];

		const [cachedInfo, cachedEntries] = await Promise.all([
			getCachedTapeInfo(id),
			getCachedLatestTapeEntries(id, TAPE_PAGE_SIZE),
		]);
		if (generation !== loadTapeGeneration) return;

		if (cachedInfo || cachedEntries.length > 0) {
			tape = cachedInfo;
			entries = cachedEntries;
			nextEntryOffset = Math.max(0, (cachedEntries[0]?.id ?? 1) - 1);
			hasMoreEntries = nextEntryOffset > 0;
			loading = false;
			await scrollTapeToLatest();
		}

		if (chat.status !== "open") {
			if (!cachedInfo && cachedEntries.length === 0) {
				loading = false;
				error = "Backend not connected";
			}
			return;
		}

		try {
			syncing = cachedInfo !== null || cachedEntries.length > 0;
			const info = await loadTapeInfo(id);
			const initialOffset = Math.max(0, info.entries - TAPE_PAGE_SIZE);
			const page = await loadTapeEntriesPage(id, initialOffset);
			if (generation !== loadTapeGeneration) return;
			tape = page.tape ?? info;
			entries = page.entries ?? cachedEntries;
			nextEntryOffset = initialOffset;
			hasMoreEntries = initialOffset > 0;
			await Promise.all([
				putCachedTapeInfo(tape),
				putCachedTapeEntries(id, entries),
			]);
			await scrollTapeToLatest();
		} catch (e) {
			if (entries.length === 0) {
				tape = null;
				entries = [];
				hasMoreEntries = false;
				nextEntryOffset = 0;
				error = e instanceof Error ? e.message : "fetch failed";
			}
		} finally {
			if (generation === loadTapeGeneration) {
				loading = false;
				syncing = false;
			}
		}
	}

	async function scrollTapeToLatest() {
		await tick();
		tapeList?.scrollToIndex(Math.max(0, entries.length - 1), { align: "end" });
	}

	async function loadTapeInfo(id: string): Promise<TapeInfo> {
		const res = await chat.runCommandAwait("tape", [id], 30_000);
		if (res.error) throw new Error(res.error);
		const data = res.data as { tape?: TapeInfo } | null | undefined;
		if (!data?.tape) throw new Error("Tape metadata missing");
		return data.tape;
	}

	async function loadTapeEntriesPage(
		id: string,
		offset: number,
		limit = TAPE_PAGE_SIZE,
	): Promise<TapeEntriesPage> {
		const res = await chat.runCommandAwait(
			"tape_entries",
			[id, String(offset), String(limit)],
			30_000,
		);
		if (res.error) throw new Error(res.error);
		return (res.data as TapeEntriesPage | null | undefined) ?? {};
	}

	async function loadMoreEntries() {
		if (!tapeId || loading || loadingMore || !hasMoreEntries) return;
		try {
			loadingMore = true;
			const previousOffset = Math.max(0, nextEntryOffset - TAPE_PAGE_SIZE);
			const limit = nextEntryOffset - previousOffset;
			const cachedEntries = await getCachedTapeEntriesPage(tapeId, previousOffset, limit);
			if (isCompletePage(cachedEntries, previousOffset, limit)) {
				entries = mergeEntries(cachedEntries, entries);
				nextEntryOffset = previousOffset;
				hasMoreEntries = previousOffset > 0;
				if (chat.status === "open") void syncEntriesPage(tapeId, previousOffset, limit);
				return;
			}
			if (chat.status !== "open") return;
			const page = await loadTapeEntriesPage(tapeId, previousOffset, limit);
			const nextEntries = page.entries ?? [];
			if (page.tape) tape = page.tape;
			if (nextEntries.length > 0) {
				entries = mergeEntries(nextEntries, entries);
				await putCachedTapeEntries(tapeId, nextEntries);
			}
			nextEntryOffset = previousOffset;
			hasMoreEntries = previousOffset > 0;
		} catch (e) {
			error = e instanceof Error ? e.message : "fetch failed";
		} finally {
			loadingMore = false;
		}
	}

	async function syncEntriesPage(id: string, offset: number, limit: number) {
		try {
			const page = await loadTapeEntriesPage(id, offset, limit);
			const syncedEntries = page.entries ?? [];
			if (page.tape) {
				tape = page.tape;
				await putCachedTapeInfo(page.tape);
			}
			if (syncedEntries.length > 0) {
				entries = mergeEntries(entries, syncedEntries);
				await putCachedTapeEntries(id, syncedEntries);
			}
		} catch {
			// Background cache refresh can fail without interrupting browsing.
		}
	}

	async function syncLatestTape(id: string) {
		if (syncing || loading || chat.status !== "open") return;
		try {
			syncing = true;
			const info = await loadTapeInfo(id);
			const initialOffset = Math.max(0, info.entries - TAPE_PAGE_SIZE);
			const page = await loadTapeEntriesPage(id, initialOffset);
			const syncedTape = page.tape ?? info;
			const syncedEntries = page.entries ?? [];
			if (tapeId === id) {
				tape = syncedTape;
				if (syncedEntries.length > 0) {
					entries = mergeEntries(entries, syncedEntries);
					nextEntryOffset = Math.max(0, (entries[0]?.id ?? 1) - 1);
				}
				hasMoreEntries = nextEntryOffset > 0;
			}
			await Promise.all([
				putCachedTapeInfo(syncedTape),
				putCachedTapeEntries(id, syncedEntries),
			]);
		} catch {
			// Keep showing cached data if background sync fails.
		} finally {
			syncing = false;
		}
	}

	function checkNeedMoreEntries() {
		if (!tapeList || !hasMoreEntries || loading || loadingMore) return;
		if (tapeList.getScrollOffset() < 1200) void loadMoreEntries();
	}

	function isCompletePage(pageEntries: TapeEntry[], offset: number, limit: number): boolean {
		if (pageEntries.length !== limit) return false;
		return pageEntries[0]?.id === offset + 1 && pageEntries.at(-1)?.id === offset + limit;
	}

	function mergeEntries(...groups: TapeEntry[][]): TapeEntry[] {
		const byId = new Map<number, TapeEntry>();
		for (const group of groups) {
			for (const entry of group) byId.set(entry.id, entry);
		}
		return [...byId.values()].sort((a, b) => a.id - b.id);
	}

	async function loadTapes(silent = false) {
		let cachedCount = 0;
		if (!silent) {
			const cached = await getCachedTapes();
			cachedCount = cached.length;
			if (cached.length > 0) {
				tapes = cached;
				loadingTapes = false;
				tapesError = null;
			}
		}
		if (chat.status !== "open") {
			if (!silent) loadingTapes = false;
			if (cachedCount === 0) tapesError = "Backend not connected";
			return;
		}

		try {
			if (!silent) loadingTapes = true;
			tapesError = null;
			const res = await chat.runCommandAwait("tapes", [], 30_000);
			if (res.error) throw new Error(res.error);
			const data = res.data as { tapes?: TapeInfo[] } | null | undefined;
			tapes = data?.tapes ?? [];
			await putCachedTapes(tapes);
		} catch (e) {
			if (!silent) {
				tapes = [];
				tapesError = e instanceof Error ? e.message : "fetch failed";
			}
		} finally {
			if (!silent) loadingTapes = false;
		}
	}

	async function deleteTape(id: string) {
		if (deletingTapeId) return;
		if (chat.status !== "open") {
			tapesError = "Backend not connected";
			return;
		}
		try {
			deletingTapeId = id;
			tapesError = null;
			const res = await chat.runCommandAwait("tape_delete", [id], 30_000);
			if (res.error) throw new Error(res.error);
			tapes = tapes.filter((item) => item.id !== id);
			await deleteCachedTape(id);
			await putCachedTapes(tapes);
			if (tapeId === id) router.go("tapes");
		} catch (e) {
			tapesError = e instanceof Error ? e.message : "delete failed";
		} finally {
			deletingTapeId = null;
			pendingDeleteTape = null;
		}
	}

	let loadedTapeId: string | null = null;
	let loadedTapesForRoute: string | null = null;
	let loadTapeGeneration = 0;
	$effect(() => {
		if (!tapeId) {
			tape = null;
			entries = [];
			hasMoreEntries = false;
			nextEntryOffset = 0;
			loadedTapeId = null;
			const routeKey = `${chat.status}:${router.current}`;
			if (loadedTapesForRoute === routeKey) return;
			loadedTapesForRoute = routeKey;
			void loadTapes();
			return;
		}
		const routeKey = `${chat.status}:${tapeId}`;
		if (routeKey === loadedTapeId) return;
		loadedTapeId = routeKey;
		void loadTape(tapeId);
	});

	onMount(() => {
		const interval = setInterval(() => {
			if (tapeId && chat.status === "open") void syncLatestTape(tapeId);
			else if (!tapeId && chat.status === "open") void loadTapes(true);
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
		if (entry.kind === "event") return String(entry.payload.name ?? "Event");
		if (entry.kind === "anchor") return "Anchor";
		if (entry.kind === "ref") return "Tape ref";
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

	function openTape(id: string) {
		router.go(`tapes/${encodeURIComponent(id)}`);
	}

	function requestDeleteTape(item: TapeInfo) {
		if (deletingTapeId) return;
		pendingDeleteTape = item;
	}

	function closeDeleteDialog() {
		if (deletingTapeId) return;
		pendingDeleteTape = null;
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
		if (entry.kind === "ref") return "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300";
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

	function valueKind(value: unknown): ToolArgRow["kind"] {
		if (value === null || value === undefined) return "null";
		if (Array.isArray(value)) return "array";
		if (typeof value === "object") return "object";
		if (typeof value === "number") return "number";
		if (typeof value === "boolean") return "boolean";
		return "string";
	}

	function formatToolArgValue(value: unknown): string {
		if (typeof value === "string") return value;
		if (value === undefined) return "undefined";
		if (value === null) return "null";
		if (typeof value === "number" || typeof value === "boolean") return String(value);
		return formatToolArgs(value);
	}

	function toolArgRows(value: unknown): ToolArgRow[] {
		if (value && typeof value === "object" && !Array.isArray(value)) {
			return Object.entries(value as Record<string, unknown>).map(([key, item]) => ({
				key,
				value: formatToolArgValue(item),
				kind: valueKind(item),
			}));
		}
		return [{
			key: "value",
			value: formatToolArgValue(value),
			kind: valueKind(value),
		}];
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
		if (entry.kind === "ref") {
			const ref = entry.payload.ref;
			if (ref && typeof ref === "object") {
				const record = ref as Record<string, unknown>;
				const meta = record.meta && typeof record.meta === "object"
					? (record.meta as Record<string, unknown>)
					: {};
				const tapeId = record.tapeId;
				if (typeof tapeId === "string") {
					return [{
						type: "tapeRef",
						tapeId,
						relation: typeof record.relation === "string" ? record.relation : undefined,
						name: typeof meta.name === "string" ? meta.name : undefined,
						kind: typeof meta.kind === "string" ? meta.kind : undefined,
						lifetime: typeof meta.lifetime === "string" ? meta.lifetime : undefined,
						agentId: typeof meta.agentId === "string" ? meta.agentId : undefined,
					}];
				}
			}
		}
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
				if (record.type === "toolCall") {
					return {
						type: "toolCall",
						id: String(record.id ?? ""),
						name: String(record.name ?? "tool"),
						args: record.arguments ?? {},
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
							<div class="border-border flex items-center gap-2 border-b px-3 py-2.5 last:border-b-0">
								<button
									type="button"
									class="hover:bg-black/[0.04] flex min-w-0 flex-1 items-center gap-3 rounded-md px-1 py-1 text-left transition-colors duration-150"
									onclick={() => router.go(`tapes/${encodeURIComponent(item.id)}`)}
									title={item.id}
								>
									<div class="bg-black/[0.04] text-muted-foreground flex size-9 shrink-0 items-center justify-center rounded-md">
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
								<button
									type="button"
									class="text-muted-foreground hover:bg-red-500/10 hover:text-red-500 flex size-8 shrink-0 items-center justify-center rounded-md transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50"
									onclick={() => requestDeleteTape(item)}
									disabled={deletingTapeId !== null}
									title="Delete tape"
									aria-label={`Delete tape ${shortTapeId(item.id)}`}
								>
									{#if deletingTapeId === item.id}
										<Icon icon={recordCircleLinear} class="size-3.5 animate-spin" />
									{:else}
										<Icon icon={trashBinTrashLinear} class="size-3.5" />
									{/if}
								</button>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		{:else if loading && entries.length === 0}
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
					shift
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
											<details class="border-border group rounded-md border bg-orange-500/[0.03] text-sm">
												<summary class="flex min-w-0 cursor-pointer list-none items-center gap-2 px-3 py-2 marker:hidden">
													<Icon
														icon={altArrowRightLinear}
														class="text-muted-foreground size-3.5 shrink-0 transition-transform duration-150 group-open:rotate-90"
													/>
													<span class="text-orange-700 dark:text-orange-300 text-[10px] font-medium uppercase tracking-wider">tool call</span>
													<span class="truncate text-xs font-medium">{block.name}</span>
													<span class="text-muted-foreground rounded bg-orange-500/10 px-1.5 py-0.5 text-[10px]">
														{toolArgRows(block.args).length} params
													</span>
													{#if block.id}
														<span class="text-muted-foreground ml-auto shrink truncate text-xs">{block.id}</span>
													{/if}
												</summary>
												<div class="border-border bg-orange-500/[0.025] border-t px-3 py-2">
													<div class="grid gap-2">
														{#if toolArgRows(block.args).length === 0}
															<div class="text-muted-foreground rounded-md bg-background px-2.5 py-2 text-xs">none</div>
														{/if}
														{#each toolArgRows(block.args) as row (row.key)}
															<div class="border-border/80 bg-background grid gap-1 rounded-md border px-2.5 py-2 md:grid-cols-[9rem_minmax(0,1fr)] md:gap-3">
																<div class="flex min-w-0 items-center gap-2">
																	<span class="truncate font-mono text-xs font-medium text-orange-700 dark:text-orange-300">{row.key}</span>
																	<span class="text-muted-foreground rounded bg-muted px-1.5 py-0.5 text-[10px]">{row.kind}</span>
																</div>
																<pre class="text-foreground/90 bg-muted/25 rounded px-2 py-1.5 whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{row.value}</pre>
															</div>
														{/each}
													</div>
												</div>
											</details>
										{:else if block.type === "tapeRef"}
											<button
												type="button"
												class="border-border hover:bg-accent/60 flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left transition-colors duration-150"
												onclick={() => openTape(block.tapeId)}
												title={block.tapeId}
											>
												<div class="min-w-0 flex-1">
													<div class="truncate text-sm font-medium">
														{block.name ?? shortTapeId(block.tapeId)}
													</div>
													<div class="text-muted-foreground mt-0.5 flex min-w-0 flex-wrap items-center gap-1.5 text-xs">
														<span>tape</span>
														{#if block.relation}
															<span>·</span>
															<span>{block.relation}</span>
														{/if}
														{#if block.kind}
															<span>·</span>
															<span>{block.kind}</span>
														{/if}
														{#if block.lifetime}
															<span>·</span>
															<span>{block.lifetime}</span>
														{/if}
														<span>·</span>
														<span class="font-mono">{shortTapeId(block.tapeId)}</span>
													</div>
												</div>
											</button>
										{:else if isMarkdown(entry)}
											<Markdown source={block.text} class="text-sm" />
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
					<div class="bg-background/90 border-border text-muted-foreground absolute left-1/2 top-3 flex -translate-x-1/2 items-center gap-2 rounded-md border px-3 py-1.5 text-xs shadow-sm">
						<Icon icon={recordCircleLinear} class="size-3.5 animate-spin" />
						<span>Loading more…</span>
					</div>
				{/if}
			</div>
		{/if}
	</div>
</section>

{#if pendingDeleteTape}
	<div class="fixed inset-0 z-50 flex items-center justify-center px-4">
		<button
			type="button"
			class="absolute inset-0 bg-black/20"
			aria-label="Cancel delete tape"
			onclick={closeDeleteDialog}
		></button>
		<div
			class="border-border bg-background relative w-full max-w-sm rounded-md border"
			role="dialog"
			aria-modal="true"
			aria-labelledby="delete-tape-title"
			tabindex="-1"
		>
			<div class="border-border border-b px-4 py-3">
				<h2 id="delete-tape-title" class="text-sm font-medium">Delete tape</h2>
				<p class="text-muted-foreground mt-1 text-xs">
					This will permanently remove this tape and its entries.
				</p>
			</div>
			<div class="px-4 py-3">
				<div class="bg-muted/40 rounded-md px-3 py-2">
					<div class="truncate font-mono text-xs">{pendingDeleteTape.id}</div>
					<div class="text-muted-foreground mt-1 text-xs">{tapeSubtitle(pendingDeleteTape)}</div>
				</div>
			</div>
			<div class="border-border flex items-center justify-end gap-2 border-t px-4 py-3">
				<button
					type="button"
					class="hover:bg-accent rounded-md px-3 py-1.5 text-sm transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50"
					onclick={closeDeleteDialog}
					disabled={deletingTapeId !== null}
				>
					Cancel
				</button>
				<button
					type="button"
					class="rounded-md bg-red-500 px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-red-600 disabled:cursor-not-allowed disabled:opacity-60"
					onclick={() => deleteTape(pendingDeleteTape!.id)}
					disabled={deletingTapeId !== null}
				>
					{#if deletingTapeId === pendingDeleteTape.id}
						Deleting…
					{:else}
						Delete
					{/if}
				</button>
			</div>
		</div>
	</div>
{/if}
