<script lang="ts">
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { signIn, signUp } from "$lib/supabase.svelte";
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
</script>

<div class="bg-background flex min-h-screen items-center justify-center">
	<div
		in:fade={{ duration: 300, easing: cubicOut }}
		class="border-border w-full max-w-sm rounded-2xl border p-8"
	>
		<h1 class="text-foreground mb-1 text-xl font-medium tracking-tight">
			{mode === "login" ? "Sign in" : "Create account"}
		</h1>
		<p class="text-muted-foreground mb-6 text-sm">
			{mode === "login"
				? "Sign in with your Supabase account."
				: "Create a new account to get started."}
		</p>

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
