<script lang="ts">
	import { Button } from "$lib/components/ui/button";
	import { settings } from "$lib/settings.svelte";
	import SettingCard from "./setting-card.svelte";
	import SettingRow from "./setting-row.svelte";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();

	function resetSettings() {
		if (!confirm("Reset all UI settings to defaults?")) return;
		settings.resetAll();
	}

	function clearChat() {
		if (!confirm("Discard the current chat transcript?")) return;
		chat.clearMessages();
	}
</script>

<section>
	<h1 class="text-foreground text-xl font-semibold tracking-tight">Advanced</h1>
	<p class="text-muted-foreground mt-1 text-sm">
		Local data and reset actions. None of these touch the daemon.
	</p>

	<SettingCard>
		<SettingRow
			label="Reset UI settings"
			desc="Clears all preferences saved in this browser."
			first
		>
			<Button variant="secondary" size="sm" class="h-8" onclick={resetSettings}>
				reset
			</Button>
		</SettingRow>

		<SettingRow
			label="Clear chat transcript"
			desc="Wipes the message history saved in this browser."
		>
			<Button variant="secondary" size="sm" class="h-8" onclick={clearChat}>
				clear
			</Button>
		</SettingRow>
	</SettingCard>

	<SettingCard title="About">
		<SettingRow label="Daemon data" desc="Where the nexal daemon stores keys, model config, and worker state." first>
			<code class="text-muted-foreground font-mono text-xs">~/.nexal/data/</code>
		</SettingRow>

		<SettingRow label="Frontend" desc="Local Svelte build connected over WebSocket to the daemon.">
			<code class="text-muted-foreground font-mono text-xs">nexal-web</code>
		</SettingRow>
	</SettingCard>
</section>
