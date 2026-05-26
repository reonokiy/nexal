<script lang="ts">
	import { createChat } from "$lib/client.svelte";
	import { router } from "$lib/router.svelte";
	import { settings, applyTheme } from "$lib/settings.svelte";
	import { auth, getAccessToken } from "$lib/supabase.svelte";
	import ChatView from "$lib/views/chat-view.svelte";
	import SettingsPage from "$lib/components/settings-page.svelte";
	import Sidebar from "$lib/components/sidebar.svelte";
	import AuthForm from "$lib/components/auth-form.svelte";
	import ComputersPage from "$lib/components/computers-page.svelte";
	import TapsPage from "$lib/components/taps-page.svelte";
	import Icon from "@iconify/svelte";
	import { sidebarCodeLinear } from "$lib/icons/solar";
	import { onMount } from "svelte";
	import { slide } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

	const chat = createChat(settings.backendUrl, settings.chatId, settings.sender, getAccessToken());

	let sidebarOpen = $state(true);
	let tapeHeader = $state<{
		id: string;
		entries: number;
		anchors: number;
		lastAnchor: string | null;
	} | null>(null);
	const loggedIn = $derived(auth.accessToken.length > 0);
	const authReady = $derived(auth.initialised);

	onMount(() => {
		applyTheme(settings.theme);
	});

	$effect(() => {
		if (chat.url === settings.backendUrl) return;
		if (loggedIn) chat.connect(settings.backendUrl);
		else chat.url = settings.backendUrl;
	});

	let wasLoggedIn = false;
	$effect(() => {
		chat.authToken = auth.accessToken;
		if (loggedIn && !wasLoggedIn) {
			wasLoggedIn = true;
			chat.connect();
		} else if (!loggedIn && wasLoggedIn) {
			wasLoggedIn = false;
			chat.disconnect();
		}
	});

	function toggleSidebar() {
		sidebarOpen = !sidebarOpen;
	}

	function setTapeHeader(info: typeof tapeHeader) {
		tapeHeader = info;
	}

	const onSettings = $derived(router.current.startsWith("settings"));
	const onComputers = $derived(router.current.startsWith("computers"));
	const onTapes = $derived(
		router.current.startsWith("tapes") || router.current.startsWith("taps"),
	);
</script>

<div class="bg-background text-foreground flex h-screen">
	{#if !authReady}
		<div class="flex h-full w-full items-center justify-center">
			<div class="border-border w-full max-w-sm rounded-2xl border p-8 text-center">
				<h1 class="text-foreground mb-1 text-xl font-medium tracking-tight">Signing you in</h1>
				<p class="text-muted-foreground text-sm">Completing the authentication callback.</p>
			</div>
		</div>
	{:else if !loggedIn}
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
		{#if onComputers || onTapes}
			<div class="flex h-screen flex-1 flex-col">
				<header class="flex h-12 items-center gap-2 px-4">
					<button
						type="button"
						aria-label={sidebarOpen ? "hide sidebar" : "show sidebar"}
						title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
					class="text-muted-foreground hover:bg-accent flex size-8 items-center justify-center rounded-md transition-colors duration-150 active:scale-90"
						onclick={toggleSidebar}
					>
						<Icon icon={sidebarCodeLinear} class="size-4" />
						<span class="sr-only">Toggle sidebar</span>
					</button>
						{#if onTapes}
							<div class="min-w-0 flex flex-1 items-center gap-3">
								<div class="flex min-w-0 items-baseline gap-2">
									<span class="text-muted-foreground shrink-0 text-xs font-medium uppercase">Tapes</span>
									{#if tapeHeader}
										<span class="truncate text-sm font-medium">{tapeHeader.id}</span>
									{/if}
								</div>
							{#if tapeHeader}
								<div class="text-muted-foreground ml-auto flex shrink-0 items-center gap-3 text-xs">
									<span>{tapeHeader.entries} entries</span>
									<span>{tapeHeader.anchors} anchors</span>
									{#if tapeHeader.lastAnchor}
										<span>last: {tapeHeader.lastAnchor}</span>
									{/if}
								</div>
							{/if}
						</div>
					{:else if onComputers}
						<div class="min-w-0 flex flex-1 items-center gap-2">
							<div class="truncate text-sm font-medium">Computers</div>
						</div>
					{/if}
				</header>
				<div class="flex min-h-0 flex-1 overflow-y-auto">
					{#if onComputers}
						<ComputersPage {chat} />
					{:else}
						<TapsPage {chat} onHeaderChange={setTapeHeader} />
					{/if}
				</div>
			</div>
		{:else}
			<ChatView {chat} {sidebarOpen} onToggleSidebar={toggleSidebar} />
		{/if}
	{/if}
</div>
