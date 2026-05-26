<script lang="ts">
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";
	import Icon from "@iconify/svelte";
	import { altArrowLeftLinear } from "$lib/icons/solar";
	import General from "$lib/components/settings/general.svelte";
	import Appearance from "$lib/components/settings/appearance.svelte";
	import ChatSection from "$lib/components/settings/chat.svelte";
	import Providers from "$lib/components/settings/providers.svelte";
	import Tools from "$lib/components/settings/tools.svelte";
	import Advanced from "$lib/components/settings/advanced.svelte";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	type SectionId =
		| "general"
		| "appearance"
		| "chat"
		| "providers"
		| "tools"
		| "advanced";

	const SECTIONS: { id: SectionId; label: string }[] = [
		{ id: "general", label: "General" },
		{ id: "appearance", label: "Appearance" },
		{ id: "chat", label: "Chat" },
		{ id: "providers", label: "Model providers" },
		{ id: "tools", label: "Tools" },
		{ id: "advanced", label: "Advanced" },
	];

	const section = $derived.by<SectionId>(() => {
		const slug = router.current.replace(/^settings\/?/, "");
		const ids: SectionId[] = SECTIONS.map((s) => s.id);
		return (ids as string[]).includes(slug)
			? (slug as SectionId)
			: "general";
	});

	function goSection(id: SectionId) {
		router.go(id === "general" ? "settings" : `settings/${id}`);
	}
</script>

<div class="bg-[#f3f3f3] text-foreground flex h-screen flex-1">
	<div class="flex min-h-0 flex-1">
		<!-- Sub-nav rail -->
		<aside
			class="flex h-full w-60 shrink-0 flex-col bg-[#f3f3f3] py-3"
		>
			<div class="px-2 pb-3">
				<button
					type="button"
					class="text-muted-foreground hover:text-foreground hover:bg-black/[0.04] flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]"
					onclick={() => router.go("home")}
				>
					<Icon icon={altArrowLeftLinear} class="size-4" />
					Back to app
				</button>
			</div>
			<nav class="flex flex-col gap-0.5 px-2 pt-3">
				{#each SECTIONS as s (s.id)}
					{@const active = section === s.id}
					<button
						type="button"
						class={cn(
							"flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
							active
								? "bg-black/[0.05] text-foreground"
								: "text-foreground/80 hover:bg-black/[0.04]",
						)}
						onclick={() => goSection(s.id)}
					>
							<span>{s.label}</span>
					</button>
				{/each}
			</nav>
		</aside>

		<!-- Active section -->
		<main class="bg-background flex-1 overflow-y-auto">
			<div
				class={cn(
					"mx-auto px-10 py-10",
					section === "providers" || section === "tools"
						? "max-w-6xl"
						: "max-w-2xl",
				)}
			>
				{#key section}
					<div in:fade={{ duration: 160, easing: cubicOut }}>
						{#if section === "general"}
							<General {chat} />
						{:else if section === "appearance"}
							<Appearance />
						{:else if section === "chat"}
							<ChatSection />
						{:else if section === "providers"}
							<Providers {chat} />
						{:else if section === "tools"}
							<Tools {chat} />
						{:else if section === "advanced"}
							<Advanced {chat} />
						{/if}
					</div>
				{/key}
			</div>
		</main>
	</div>
</div>
