<script lang="ts">
	import { createChat } from "$lib/client.svelte";
	import { router } from "$lib/router.svelte";
	import { settings, applyTheme } from "$lib/settings.svelte";
	import ChatView from "$lib/views/chat-view.svelte";
	import SettingsPage from "$lib/components/settings-page.svelte";
	import Sidebar from "$lib/components/sidebar.svelte";
	import { onMount } from "svelte";
	import { slide } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

	const chat = createChat(settings.backendUrl, settings.chatId, settings.sender);

	let sidebarOpen = $state(true);

	onMount(() => {
		applyTheme(settings.theme);
		chat.connect();
	});

	$effect(() => {
		if (chat.url !== settings.backendUrl) chat.url = settings.backendUrl;
	});

	function toggleSidebar() {
		sidebarOpen = !sidebarOpen;
	}

	const onSettings = $derived(router.current.startsWith("settings"));
</script>

<div class="bg-background text-foreground flex h-screen">
	{#if onSettings}
		<SettingsPage {chat} />
	{:else}
		{#if sidebarOpen}
			<div
				transition:slide={{ axis: "x", duration: 200, easing: cubicOut }}
				class="overflow-hidden"
			>
				<Sidebar {chat} />
			</div>
		{/if}
		<ChatView {chat} {sidebarOpen} onToggleSidebar={toggleSidebar} />
	{/if}
</div>
