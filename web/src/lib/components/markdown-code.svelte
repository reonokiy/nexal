<script lang="ts">
	import DOMPurify from "dompurify";
	import { highlightCode, resolveShikiLanguage } from "$lib/shiki-highlight";

	interface Props {
		lang?: string;
		text: string;
	}

	let { lang = "", text }: Props = $props();

	let highlighted = $state<string | null>(null);
	let failed = $state(false);

	$effect(() => {
		const source = text;
		const currentLang = lang;
		let cancelled = false;

		highlighted = null;
		failed = false;

		if (!currentLang.trim()) return;

		const resolvedLanguage = resolveShikiLanguage(currentLang);
		if (!resolvedLanguage) return;

		highlightCode(source, resolvedLanguage)
			.then((html) => {
				if (!cancelled && html) highlighted = DOMPurify.sanitize(html);
			})
			.catch(() => {
				if (!cancelled) failed = true;
			});

		return () => {
			cancelled = true;
		};
	});
</script>

<figure class="md-code">
	{#if lang}
		<figcaption>{lang}</figcaption>
	{/if}
	{#if highlighted && !failed}
		{@html highlighted}
	{:else}
		<pre><code>{text}</code></pre>
	{/if}
</figure>
