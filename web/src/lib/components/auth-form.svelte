<script lang="ts">
	import { auth, signInWithProvider } from "$lib/supabase.svelte";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";
	import { onMount } from "svelte";

	let error = $state<string | null>(null);
	let busy = $state(false);

	function readAuthErrorFromUrl(): string | null {
		if (typeof window === "undefined") return null;
		const sources: URLSearchParams[] = [];
		if (window.location.hash.startsWith("#")) {
			sources.push(new URLSearchParams(window.location.hash.slice(1)));
		}
		if (window.location.search) {
			sources.push(new URLSearchParams(window.location.search));
		}
		for (const params of sources) {
			const code = params.get("error_code");
			const desc = params.get("error_description");
			const err = params.get("error");
			if (code || desc || err) {
				if (code === "signup_disabled") {
					return "This GitHub account isn't allowed to sign up. Ask an admin to grant access.";
				}
				return desc ? desc.replace(/\+/g, " ") : err || code || "Login failed.";
			}
		}
		return null;
	}

	onMount(() => {
		const e = readAuthErrorFromUrl();
		if (e && !auth.error) {
			error = e;
			const url = new URL(window.location.href);
			url.hash = "";
			url.search = "";
			window.history.replaceState({}, "", url.toString());
		}
		if (auth.error) error = auth.error;
	});

	async function githubLogin() {
		busy = true;
		error = null;
		const result = await signInWithProvider("github");
		if (result.error) error = result.error;
		busy = false;
	}
</script>

<div class="flex h-full w-full items-center justify-center">
	<div
		in:fade={{ duration: 300, easing: cubicOut }}
		class="border-border w-full max-w-sm rounded-2xl border p-8"
	>
		<h1 class="text-foreground mb-1 text-xl font-medium tracking-tight">
			Sign in to nexal
		</h1>
		<p class="text-muted-foreground mb-6 text-sm">
			Continue with your GitHub account.
		</p>

		<button type="button" onclick={githubLogin} disabled={busy}
			class="border-border hover:bg-accent flex w-full items-center justify-center gap-2 rounded-lg border px-4 py-2.5 text-sm transition-colors disabled:opacity-50">
			<svg class="size-4" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0 0 24 12c0-6.63-5.37-12-12-12z"/></svg>
			{busy ? "Please wait…" : "Continue with GitHub"}
		</button>

		{#if error}
			<div
				in:fade={{ duration: 150 }}
				class="border-destructive/40 bg-destructive/5 text-destructive mt-4 rounded-lg border px-3 py-2 text-xs"
			>
				{error}
			</div>
		{/if}
	</div>
</div>
