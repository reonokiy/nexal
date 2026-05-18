<script lang="ts">
	import { createChat } from "$lib/client.svelte";
	import { router } from "$lib/router.svelte";
	import { settings, applyTheme } from "$lib/settings.svelte";
	import { getAccessToken, isLoggedIn } from "$lib/supabase.svelte";
	import ChatView from "$lib/views/chat-view.svelte";
	import SettingsPage from "$lib/components/settings-page.svelte";
	import Sidebar from "$lib/components/sidebar.svelte";
	import AuthForm from "$lib/components/auth-form.svelte";
	import { onMount } from "svelte";
	import { slide } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

	const chat = createChat(settings.backendUrl, settings.chatId, settings.sender, getAccessToken());

	let sidebarOpen = $state(true);
	let loggedIn = $state(isLoggedIn());

	onMount(() => {
		applyTheme(settings.theme);
		if (loggedIn) chat.connect();
	});

	$effect(() => {
		if (chat.url !== settings.backendUrl) chat.url = settings.backendUrl;
	});

	// Watch auth state: when user logs in, connect; when they log out, disconnect.
	$effect(() => {
		const token = getAccessToken();
		const now = !!token;
		if (now !== loggedIn) {
			loggedIn = now;
			if (now) {
				chat.connect();
			} else {
				chat.disconnect();
			}
		}
	});

	function toggleSidebar() {
		sidebarOpen = !sidebarOpen;
	}

	const onSettings = $derived(router.current.startsWith("settings"));
</script>

<div class="bg-background text-foreground flex h-screen">
	{#if !loggedIn}
		<AuthForm />
	{:else if onSettings}
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
