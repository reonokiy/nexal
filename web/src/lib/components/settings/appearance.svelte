<script lang="ts">
	import { settings } from "$lib/settings.svelte";
	import SettingCard from "./setting-card.svelte";
	import SettingRow from "./setting-row.svelte";
	import Toggle from "./toggle.svelte";
	import Segmented from "./segmented.svelte";
</script>

<section>
	<h1 class="text-foreground text-xl font-semibold tracking-tight">
		Appearance
	</h1>
	<p class="text-muted-foreground mt-1 text-sm">
		Theme, density, and what appears alongside each message.
	</p>

	<SettingCard>
		<SettingRow label="Theme" desc="Light, dark, or follow the system." first>
			<Segmented
				value={settings.theme}
				onchange={(v) => (settings.theme = v)}
				options={[
					{ value: "light", label: "Light" },
					{ value: "dark", label: "Dark" },
					{ value: "system", label: "System" },
				]}
			/>
		</SettingRow>

		<SettingRow
			label="Compact density"
			desc="Tighter spacing in chat and side panels."
		>
			<Toggle
				checked={settings.compact}
				onchange={(v) => (settings.compact = v)}
				label="Compact density"
			/>
		</SettingRow>

		<SettingRow
			label="Show timestamps"
			desc="Display the time next to each message."
		>
			<Toggle
				checked={settings.showTimestamps}
				onchange={(v) => (settings.showTimestamps = v)}
				label="Show timestamps"
			/>
		</SettingRow>
	</SettingCard>
</section>
