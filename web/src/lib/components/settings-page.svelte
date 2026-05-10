<script lang="ts">
	import { router } from "$lib/router.svelte";
	import { cn } from "$lib/utils";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";
	import ArrowLeft from "@lucide/svelte/icons/arrow-left";
	import Settings from "@lucide/svelte/icons/settings";
	import Palette from "@lucide/svelte/icons/palette";
	import MessageSquare from "@lucide/svelte/icons/message-square";
	import Sparkles from "@lucide/svelte/icons/sparkles";
	import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
	import General from "$lib/components/settings/general.svelte";
	import Appearance from "$lib/components/settings/appearance.svelte";
	import ChatSection from "$lib/components/settings/chat.svelte";
	import Providers from "$lib/components/settings/providers.svelte";
	import Advanced from "$lib/components/settings/advanced.svelte";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	type SectionId =
		| "general"
		| "appearance"
		| "chat"
		| "providers"
		| "advanced";

	const SECTIONS: { id: SectionId; label: string; icon: any }[] = [
		{ id: "general", label: "General", icon: Settings },
		{ id: "appearance", label: "Appearance", icon: Palette },
		{ id: "chat", label: "Chat", icon: MessageSquare },
		{ id: "providers", label: "Model providers", icon: Sparkles },
		{ id: "advanced", label: "Advanced", icon: SlidersHorizontal },
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

<div class="bg-background text-foreground flex h-screen flex-1">
	<!-- Sub-nav rail -->
	<aside
		class="border-border bg-muted/20 flex h-full w-60 shrink-0 flex-col border-r"
	>
		<button
			type="button"
			class="text-muted-foreground hover:text-foreground hover:bg-accent mx-2 mt-3 flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]"
			onclick={() => router.go("home")}
		>
			<ArrowLeft class="size-4" />
			Back to app
		</button>

		<nav class="mt-4 flex flex-col gap-0.5 px-2">
			{#each SECTIONS as s (s.id)}
				{@const active = section === s.id}
				<button
					type="button"
					class={cn(
						"flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-sm transition-colors duration-150 active:scale-[0.98]",
						active
							? "bg-accent text-foreground"
							: "text-foreground/85 hover:bg-accent/60",
					)}
					onclick={() => goSection(s.id)}
				>
					<s.icon class="size-4" />
					<span>{s.label}</span>
				</button>
			{/each}
		</nav>
	</aside>

	<!-- Active section -->
	<main class="flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl px-8 py-10">
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
					{:else if section === "advanced"}
						<Advanced {chat} />
					{/if}
				</div>
			{/key}
		</div>
	</main>
</div>
