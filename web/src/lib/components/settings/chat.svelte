<script lang="ts">
	import { settings } from "$lib/settings.svelte";
	import { Input } from "$lib/components/ui/input";
	import SettingCard from "./setting-card.svelte";
	import SettingRow from "./setting-row.svelte";
	import Toggle from "./toggle.svelte";
</script>

<section>
	<h1 class="text-foreground text-2xl font-medium tracking-tight">Chat</h1>
	<p class="text-muted-foreground mt-1 text-sm">
		Defaults for the chat surface and what gets sent to the daemon.
	</p>

	<SettingCard>
		<SettingRow
			label="Show system messages"
			desc="Connection notes, command results, and other meta lines."
			first
		>
			<Toggle
				checked={settings.showSystem}
				onchange={(v) => (settings.showSystem = v)}
				label="Show system messages"
			/>
		</SettingRow>

		<SettingRow
			label="Default chat id"
			desc="Sent as chat_id on every message. Affects which session the daemon uses."
		>
			<Input
				class="h-9 w-48 font-mono text-xs"
				value={settings.chatId}
				oninput={(e) => (settings.chatId = (e.currentTarget as HTMLInputElement).value)}
			/>
		</SettingRow>

		<SettingRow
			label="Default sender"
			desc="Display name attached to your outgoing messages."
		>
			<Input
				class="h-9 w-48 text-xs"
				value={settings.sender}
				oninput={(e) => (settings.sender = (e.currentTarget as HTMLInputElement).value)}
			/>
		</SettingRow>
	</SettingCard>
</section>
