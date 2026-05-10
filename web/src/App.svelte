<script lang="ts">
	import { createChat } from "$lib/client.svelte";
	import { router } from "$lib/router.svelte";
	import ChatView from "$lib/views/chat-view.svelte";
	import SettingsPage from "$lib/components/settings-page.svelte";
	import Sidebar from "$lib/components/sidebar.svelte";
	import { onMount } from "svelte";

	const defaultUrl =
		(import.meta.env.VITE_NEXAL_BACKEND as string | undefined) ??
		"ws://127.0.0.1:3001";
	const stored =
		typeof localStorage !== "undefined"
			? localStorage.getItem("nexal.backend")
			: null;

	const chat = createChat(stored ?? defaultUrl);

	onMount(() => chat.connect());

	$effect(() => {
		try {
			localStorage.setItem("nexal.backend", chat.url);
		} catch {}
	});
</script>

<svelte:head>
	<script>
		if (
			window.matchMedia &&
			window.matchMedia("(prefers-color-scheme: dark)").matches
		) {
			document.documentElement.classList.add("dark");
		}
	</script>
</svelte:head>

<div class="bg-background text-foreground flex h-screen">
	<Sidebar {chat} />
	{#if router.current === "settings"}
		<SettingsPage {chat} />
	{:else}
		<ChatView {chat} />
	{/if}
</div>
