<script lang="ts">
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { signIn, signUp, signInWithProvider } from "$lib/supabase.svelte";
	import { fade } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

	let email = $state("");
	let password = $state("");
	let error = $state<string | null>(null);
	let busy = $state(false);
	let mode = $state<"login" | "signup">("login");

	async function submit() {
		if (!email.trim() || !password.trim()) return;
		busy = true;
		error = null;
		const result = mode === "login" ? await signIn(email, password) : await signUp(email, password);
		if (result.error) error = result.error;
		busy = false;
	}

	async function oidcLogin(provider: "google" | "github") {
		busy = true;
		error = null;
		const result = await signInWithProvider(provider);
		if (result.error) error = result.error;
		busy = false;
	}
</script>

<div class="bg-background flex min-h-screen items-center justify-center">
	<div
		in:fade={{ duration: 300, easing: cubicOut }}
		class="border-border w-full max-w-sm rounded-2xl border p-8"
	>
		<h1 class="text-foreground mb-1 text-xl font-medium tracking-tight">
			Sign in to nexal
		</h1>
		<p class="text-muted-foreground mb-6 text-sm">
			Use your account to continue.
		</p>

		<!-- OIDC Buttons -->
		<div class="flex flex-col gap-2.5 mb-5">
			<button type="button" onclick={() => oidcLogin("google")} disabled={busy}
				class="border-border hover:bg-accent flex items-center justify-center gap-2 rounded-lg border px-4 py-2.5 text-sm transition-colors disabled:opacity-50">
				<svg class="size-4" viewBox="0 0 24 24"><path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z"/><path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/><path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/><path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/></svg>
				Google
			</button>
			<button type="button" onclick={() => oidcLogin("github")} disabled={busy}
				class="border-border hover:bg-accent flex items-center justify-center gap-2 rounded-lg border px-4 py-2.5 text-sm transition-colors disabled:opacity-50">
				<svg class="size-4" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0 0 24 12c0-6.63-5.37-12-12-12z"/></svg>
				GitHub
			</button>
		</div>

		<div class="mb-4 flex items-center gap-3">
			<hr class="border-border flex-1" />
			<span class="text-muted-foreground text-xs">or continue with email</span>
			<hr class="border-border flex-1" />
		</div>

		<form onsubmit={(e) => { e.preventDefault(); submit(); }} class="flex flex-col gap-4">
			<label class="flex flex-col gap-1.5">
				<span class="text-muted-foreground text-xs">Email</span>
				<Input
					type="email"
					placeholder="you@example.com"
					bind:value={email}
					disabled={busy}
				/>
			</label>
			<label class="flex flex-col gap-1.5">
				<span class="text-muted-foreground text-xs">Password</span>
				<Input
					type="password"
					placeholder="••••••••"
					bind:value={password}
					disabled={busy}
				/>
			</label>

			{#if error}
				<div
					in:fade={{ duration: 150 }}
					class="border-destructive/40 bg-destructive/5 text-destructive rounded-lg border px-3 py-2 text-xs"
				>
					{error}
				</div>
			{/if}

			<Button type="submit" disabled={busy} class="w-full">
				{busy ? "Please wait…" : mode === "login" ? "Sign in" : "Create account"}
			</Button>
		</form>

		<p class="text-muted-foreground mt-4 text-center text-xs">
			{mode === "login" ? "No account yet?" : "Already have an account?"}{" "}
			<button
				type="button"
				class="text-foreground underline underline-offset-2"
				onclick={() => { mode = mode === "login" ? "signup" : "login"; error = null; }}
			>
				{mode === "login" ? "Sign up" : "Sign in"}
			</button>
		</p>
	</div>
</div>
