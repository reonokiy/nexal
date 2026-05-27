<script lang="ts">
	import { tick, onMount } from "svelte";
	import { VList, type VListHandle } from "virtua/svelte";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";
	import Message from "$lib/components/message.svelte";
	import type { Source } from "$lib/components/message.svelte";
	import Composer from "$lib/components/composer.svelte";
	import EmptyState from "$lib/components/empty-state.svelte";
	import Icon from "@iconify/svelte";
	import {
		plugCircleLinear,
		sidebarCodeLinear,
	} from "$lib/icons/solar";
	import { settings } from "$lib/settings.svelte";
	import type { Chat, Message as Msg } from "$lib/client.svelte";

	interface Props {
		chat: Chat;
		sidebarOpen: boolean;
		onToggleSidebar: () => void;
	}
	let { chat, sidebarOpen, onToggleSidebar }: Props = $props();

	let input = $state("");
	let modelLabel = $state("model");

	interface MsgItem {
		id: number;
		streamId?: string;
		role: "user" | "agent";
		text: string;
		ts: number;
		streaming?: boolean;
		kind: "msg";
		source?: Source;
		toolName?: string;
		workerId?: string;
		workerStatus?: string;
		workerName?: string;
		workerKind?: string;
		workerLifetime?: string;
		workerPath?: string;
	}

	type DisplayItem = MsgItem | { kind: "typing"; id: number };

	interface WorkerMeta {
		name?: string;
		kind?: string;
		lifetime?: string;
	}

	interface SpawnEvent {
		id: string;
		name: string;
		kind: string;
		lifetime: string;
		status: string;
	}

	interface TaskItem {
		id: string;
		name: string;
		kind: string;
		lifetime?: string;
		status: string;
		lastText: string;
		ts: number;
		updates: number;
	}

	interface ChildTransfer {
		child: string;
		text: string;
	}

	/** Parse source from message text (short-term until backend sends metadata). */
	function parseSource(text: string): {
		source?: Source;
		text: string;
		toolName?: string;
		workerId?: string;
		workerStatus?: string;
	} {
		const toolMatch = text.match(/^\[tool:([\w-]+)\]\s*/);
		if (toolMatch) {
			return {
				source: "tool",
				toolName: toolMatch[1],
				text: text.slice(toolMatch[0].length),
			};
		}
		const workerMatch = text.match(/^\[worker:([^\]]+)\]\s*status:([\w-]+)\s*/);
		if (workerMatch) {
			return {
				source: "worker",
				workerId: workerMatch[1],
				workerStatus: workerMatch[2],
				text: text.slice(workerMatch[0].length),
			};
		}
		return { text };
	}

	function parseChildTransfer(text: string): ChildTransfer | undefined {
		const match = text.match(/^\[from child ([^\]]+)\]\s*/);
		if (!match) return undefined;
		return {
			child: match[1]!,
			text: text.slice(match[0].length),
		};
	}

	function workerFromMetadata(metadata: Msg["metadata"]): WorkerMeta | undefined {
		const worker = metadata?.worker;
		if (!worker || typeof worker !== "object") return undefined;
		return worker;
	}

	function parseSpawn(text: string): SpawnEvent | undefined {
		const match = text.match(
			/\bspawned\s+([\w-]+)\s+\(([\w-]+)\)\s+id=([^\s]+)\s+name=([^\s]+)\s+status=([^\s]+)/i,
		);
		if (!match) return undefined;
		return {
			kind: match[1]!,
			lifetime: match[2]!,
			id: match[3]!,
			name: match[4]!,
			status: match[5]!,
		};
	}

	function inferWorkerStatus(text: string, existing?: string): string {
		const lower = text.toLowerCase();
		if (text.includes("❌") || lower.includes("failed:") || lower.includes("error:")) {
			return "failed";
		}
		if (lower.includes("complete") || lower.includes("finished") || lower.includes("done")) {
			return "done";
		}
		return existing === "spawning" ? "running" : existing ?? "running";
	}

	function upsertTask(
		tasks: Map<string, TaskItem>,
		task: Omit<TaskItem, "updates">,
	): void {
		const prev = tasks.get(task.id);
		tasks.set(task.id, {
			...prev,
			...task,
			updates: (prev?.updates ?? 0) + 1,
			lastText: task.lastText || prev?.lastText || "",
		});
	}

	const visibleMessages = $derived(
		chat.messages.filter((m) => m.role !== "system"),
	);

	const displayItems = $derived.by<DisplayItem[]>(() => {
		const out: DisplayItem[] = visibleMessages.map((m) => {
			if (m.role === "agent") {
				const parsed = parseSource(m.text);
				const worker = workerFromMetadata(m.metadata);
				const transfer = parseChildTransfer(parsed.text);
				const workerName = worker?.name ?? parsed.workerId ?? transfer?.child;
				const workerPath = transfer
					? `${transfer.child} > coordinator`
					: workerName
						? `coordinator > ${workerName}`
						: undefined;
				return {
					id: m.id,
					streamId: m.streamId,
					role: m.role,
					text: transfer?.text ?? parsed.text,
					ts: m.ts,
					streaming: m.streaming,
					kind: "msg" as const,
					source: parsed.source ?? (worker || transfer ? "worker" : "coordinator"),
					toolName: parsed.toolName,
					workerId: parsed.workerId ?? workerName,
					workerStatus: parsed.workerStatus ?? (worker ? inferWorkerStatus(parsed.text) : undefined),
					workerName: worker?.name,
					workerKind: worker?.kind,
					workerLifetime: worker?.lifetime,
					workerPath,
				};
			}
			// visibleMessages filters out system, so m.role must be "user" here
			return {
				id: m.id,
				streamId: m.streamId,
				role: m.role as "user",
				text: m.text,
				ts: m.ts,
				streaming: m.streaming,
				kind: "msg" as const,
			};
		});
		if (chat.typing && !chat.messages.some((m) => m.streaming)) {
			out.push({ kind: "typing", id: -1 });
		}
		return out;
	});

	const taskItems = $derived.by<TaskItem[]>(() => {
		const tasks = new Map<string, TaskItem>();
		const nameToId = new Map<string, string>();

		for (const message of visibleMessages) {
			if (message.role !== "agent") continue;
			const parsed = parseSource(message.text);
			const transfer = parseChildTransfer(parsed.text);
			const spawn = parseSpawn(parsed.text);
			if (spawn) {
				nameToId.set(spawn.name, spawn.id);
				upsertTask(tasks, {
					id: spawn.id,
					name: spawn.name,
					kind: spawn.kind,
					lifetime: spawn.lifetime,
					status: spawn.status,
					lastText: "",
					ts: message.ts,
				});
			}

			const worker = workerFromMetadata(message.metadata);
			if (!worker && parsed.source !== "worker" && !transfer) continue;

			const name = worker?.name ?? parsed.workerId ?? transfer?.child ?? "worker";
			const id = parsed.workerId ?? nameToId.get(name) ?? name;
			const existing = tasks.get(id);
			upsertTask(tasks, {
				id,
				name,
				kind: worker?.kind ?? existing?.kind ?? "worker",
				lifetime: worker?.lifetime ?? existing?.lifetime,
				status: parsed.workerStatus ?? inferWorkerStatus(transfer?.text ?? parsed.text, existing?.status),
				lastText: transfer?.text ?? parsed.text,
				ts: message.ts,
			});
			nameToId.set(name, id);
		}

		return [...tasks.values()].sort((a, b) => b.ts - a.ts);
	});

	const taskSummary = $derived.by(() => {
		let active = 0;
		let failed = 0;
		let done = 0;
		for (const task of taskItems) {
			if (task.status === "failed") failed++;
			else if (["done", "completed", "suspended", "cancelled"].includes(task.status)) done++;
			else active++;
		}
		return { active, failed, done, total: taskItems.length };
	});

	const empty = $derived(
		chat.messages.filter((m) => m.role !== "system").length === 0,
	);

	let vlist: VListHandle | undefined = $state();
	let stickToBottom = $state(true);

	function onScroll(offset: number) {
		if (!vlist) return;
		const max = vlist.getScrollSize() - vlist.getViewportSize();
		stickToBottom = max - offset < 80;
	}

	$effect(() => {
		const last = displayItems[displayItems.length - 1];
		void displayItems.length;
		if (last && "text" in last) void last.text;
		if (!stickToBottom || !vlist || displayItems.length === 0) return;
		const idx = displayItems.length - 1;
		tick().then(() => vlist?.scrollToIndex(idx, { align: "end" }));
	});

	function send() {
		if (!input.trim()) return;
		chat.sendText(input);
		input = "";
		stickToBottom = true;
	}

	function newChat() {
		chat.messages.length = 0;
		input = "";
	}

	function statusClass(status: string): string {
		if (status === "failed") return "text-red-500 bg-red-500/10";
		if (["done", "completed"].includes(status)) return "text-emerald-500 bg-emerald-500/10";
		if (["suspended", "cancelled"].includes(status)) return "text-muted-foreground bg-muted";
		return "text-primary bg-primary/10";
	}

	function shortId(id: string): string {
		return id.length > 12 ? id.slice(0, 8) : id;
	}

	function taskPath(task: TaskItem): string {
		return `coordinator > ${task.name}`;
	}

	async function refreshModelLabel() {
		try {
			const res = await chat.runCommandAwait("providers", []);
			const data = res.data as
				| { active: { provider: string; modelId: string } | null }
				| null
				| undefined;
			if (data?.active) {
				modelLabel = `${data.active.provider} / ${data.active.modelId}`;
			} else {
				modelLabel = "no model";
			}
		} catch {
			// not connected yet — retry below
		}
	}

	onMount(() => {
		const tryLoad = () => {
			if (chat.status === "open") void refreshModelLabel();
			else setTimeout(tryLoad, 400);
		};
		tryLoad();
	});
</script>

<div class="flex h-screen flex-1 flex-col">
	<header class="flex h-12 items-center gap-2 px-4">
		<button
			type="button"
			aria-label={sidebarOpen ? "hide sidebar" : "show sidebar"}
			title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
			class="text-muted-foreground hover:bg-accent flex size-8 items-center justify-center rounded-md transition-colors duration-150 active:scale-90"
			onclick={onToggleSidebar}
		>
			<Icon icon={sidebarCodeLinear} class="size-4" />
		</button>
		<button
			type="button"
			class="text-foreground/85 hover:bg-accent rounded-md px-2 py-1 text-sm font-medium transition-colors duration-150 active:scale-[0.97]"
			onclick={newChat}
			title="Clear chat"
		>
			Chat
		</button>
		<div class="ml-auto flex items-center gap-1">
			{#if taskSummary.total > 0}
				<div
					class="text-muted-foreground hidden items-center gap-2 rounded-md px-2 py-1 text-xs sm:flex"
					title="Coordinator subtasks"
				>
					<span>{taskSummary.active} active</span>
					{#if taskSummary.failed > 0}
						<span class="text-red-500">{taskSummary.failed} failed</span>
					{/if}
				</div>
			{/if}
			{#if chat.status !== "open"}
				<button
					type="button"
					in:fade={{ duration: 150 }}
					out:fade={{ duration: 100 }}
					class="border-border hover:bg-accent flex items-center gap-1.5 rounded-md border px-3 py-1 text-sm transition-colors duration-150 active:scale-[0.97]"
					onclick={() => chat.connect(chat.url)}
					title="Reconnect to backend"
				>
					<Icon icon={plugCircleLinear} class="size-3.5" />
					reconnect
				</button>
			{/if}
		</div>
	</header>

	<main class="flex min-h-0 flex-1">
		<section class="flex min-w-0 flex-1 flex-col">
			{#if taskItems.length > 0}
				<div class="border-border/70 border-y px-4 py-2 lg:hidden">
					<div class="mx-auto flex w-full max-w-3xl gap-2 overflow-x-auto">
						{#each taskItems.slice(0, 6) as task (task.id)}
							<div class="bg-muted/40 flex min-w-48 items-center gap-2 rounded-md px-2.5 py-2">
								<div class="min-w-0 flex-1">
									<div class="truncate text-xs font-medium">{taskPath(task)}</div>
									<div class="text-muted-foreground truncate text-[11px]">
										{task.kind}
										{#if task.lifetime}
											· {task.lifetime}
										{/if}
									</div>
								</div>
								<span class={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${statusClass(task.status)}`}>
									{task.status}
								</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}
			{#if empty}
				<div
					class="flex flex-1 overflow-y-auto"
					in:fade={{ duration: 200, easing: cubicOut }}
					out:fade={{ duration: 120 }}
				>
					<EmptyState />
				</div>
			{:else}
				<VList
					bind:this={vlist}
					data={displayItems}
					getKey={(item: DisplayItem) => item.id}
					onscroll={onScroll}
					style="height: 100%; width: 100%;"
				>
					{#snippet children(item: DisplayItem)}
						<div class="mx-auto w-full max-w-3xl px-4">
							{#if item.kind === "typing"}
								<div class="flex items-center gap-1.5 py-3">
									<span class="bg-foreground/40 size-1.5 animate-bounce rounded-full"></span>
									<span
										class="bg-foreground/40 size-1.5 animate-bounce rounded-full"
										style="animation-delay: 0.15s"
									></span>
									<span
										class="bg-foreground/40 size-1.5 animate-bounce rounded-full"
										style="animation-delay: 0.3s"
									></span>
								</div>
							{:else}
								<Message
									role={item.role}
									source={item.source}
									text={item.text}
									ts={item.ts}
									streaming={item.streaming ?? false}
									toolName={item.toolName}
									workerId={item.workerId}
									workerStatus={item.workerStatus}
									workerPath={item.workerPath}
								/>
							{/if}
						</div>
					{/snippet}
				</VList>
			{/if}
		</section>
		{#if taskItems.length > 0}
			<aside class="border-border/70 hidden w-80 shrink-0 border-l lg:flex lg:flex-col">
				<div class="border-border/70 flex h-12 items-center gap-2 border-b px-4">
					<div class="text-sm font-medium">Tasks</div>
					<div class="text-muted-foreground ml-auto text-xs">
						{taskSummary.active} active
					</div>
				</div>
				<div class="min-h-0 flex-1 overflow-y-auto">
					{#each taskItems as task (task.id)}
						<div class="border-border/60 border-b px-4 py-3">
							<div class="min-w-0 flex-1">
								<div class="flex items-center gap-2">
									<div class="truncate text-sm font-medium">{taskPath(task)}</div>
									<span class={`inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium ${statusClass(task.status)}`}>
										{task.status}
									</span>
								</div>
								<div class="text-muted-foreground mt-1 flex items-center gap-1.5 text-xs">
									<span>{task.kind}</span>
									{#if task.lifetime}
										<span>·</span>
										<span>{task.lifetime}</span>
									{/if}
									<span>·</span>
									<span class="font-mono">{shortId(task.id)}</span>
								</div>
								{#if task.lastText}
									<div class="text-muted-foreground mt-2 line-clamp-3 text-xs leading-relaxed">
										{task.lastText}
									</div>
								{/if}
							</div>
						</div>
					{/each}
				</div>
			</aside>
		{/if}
	</main>

	<div class="px-4 pb-4 pt-2">
		<div class="mx-auto w-full max-w-3xl">
			<Composer {chat} bind:value={input} onSubmit={send} {modelLabel} onModelChange={refreshModelLabel} />
		</div>
	</div>
</div>
