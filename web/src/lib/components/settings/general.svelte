<script lang="ts">
	import { settings } from "$lib/settings.svelte";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import SettingCard from "./setting-card.svelte";
	import SettingRow from "./setting-row.svelte";
	import Toggle from "./toggle.svelte";
	import Segmented from "./segmented.svelte";
	import type { Chat } from "$lib/client.svelte";

	let { chat }: { chat: Chat } = $props();
	let urlDraft = $state(settings.backendUrl);

	function applyUrl() {
		const v = urlDraft.trim();
		if (!v) return;
		settings.backendUrl = v;
		chat.connect(v);
	}
</script>

<section>
	<h1 class="text-foreground text-xl font-semibold tracking-tight">General</h1>
	<p class="text-muted-foreground mt-1 text-sm">
		How nexal connects and how you interact with it.
	</p>

	<SettingCard>
		<SettingRow
			label="Backend WebSocket URL"
			desc="Where the nexal daemon is listening. Apply reconnects."
			first
		>
			<Input class="h-8 w-64 font-mono text-xs" bind:value={urlDraft} />
			<Button variant="secondary" size="sm" class="h-8" onclick={applyUrl}>
				apply
			</Button>
		</SettingRow>

		<SettingRow
			label="Send shortcut"
			desc="Press Enter to send, or require Shift+Enter."
		>
			<Segmented
				value={settings.sendKey}
				onchange={(v) => (settings.sendKey = v)}
				options={[
					{ value: "enter", label: "Enter" },
					{ value: "shift-enter", label: "Shift+Enter" },
				]}
			/>
		</SettingRow>

		<SettingRow
			label="Auto reconnect"
			desc="Try to re-open the WebSocket if the daemon drops."
		>
			<Toggle
				checked={settings.autoReconnect}
				onchange={(v) => (settings.autoReconnect = v)}
				label="Auto reconnect"
			/>
		</SettingRow>
	</SettingCard>
</section>
