<script lang="ts">
	import { cn } from "$lib/utils";

	interface Props {
		role: "user" | "agent";
		text: string;
		ts: number;
	}
	let { role, text, ts }: Props = $props();

	function fmt(t: number) {
		const d = new Date(t);
		return `${d.getHours().toString().padStart(2, "0")}:${d
			.getMinutes()
			.toString()
			.padStart(2, "0")}`;
	}
</script>

<div class={cn("flex w-full gap-2", role === "user" && "flex-row-reverse")}>
	<div
		class={cn(
			"text-muted-foreground flex size-8 shrink-0 items-center justify-center rounded-full text-xs font-semibold",
			role === "user" ? "bg-primary text-primary-foreground" : "bg-muted",
		)}
	>
		{role === "user" ? "you" : "n"}
	</div>
	<div class={cn("flex max-w-[75%] flex-col gap-1", role === "user" && "items-end")}>
		<div
			class={cn(
				"rounded-2xl px-4 py-2 text-sm leading-relaxed whitespace-pre-wrap break-words",
				role === "user"
					? "bg-primary text-primary-foreground rounded-br-sm"
					: "bg-muted text-foreground rounded-bl-sm",
			)}
		>
			{text}
		</div>
		<span class="text-muted-foreground px-1 text-[10px]">{fmt(ts)}</span>
	</div>
</div>
