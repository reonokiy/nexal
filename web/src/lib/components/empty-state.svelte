<script lang="ts">
	import { fade, fly } from "svelte/transition";
	import { cubicOut } from "svelte/easing";

</script>

<div
	class="mx-auto flex w-full max-w-3xl flex-1 flex-col px-4"
	in:fade={{ duration: 220, easing: cubicOut }}
>
	<div
		class="flex flex-1 flex-col items-center justify-center"
		in:fly={{ y: 6, duration: 260, easing: cubicOut }}
	>
		<div class="nexal-lockup relative flex flex-col items-center">
			<div class="nexal-mark" aria-hidden="true">
				<span class="nexal-mark__frame"></span>
				<span class="nexal-mark__halo"></span>
				<span class="nexal-mark__rail nexal-mark__rail--left"></span>
				<span class="nexal-mark__rail nexal-mark__rail--right"></span>
				<span class="nexal-mark__beam"></span>
				<span class="nexal-mark__beam nexal-mark__beam--echo"></span>
				<span class="nexal-mark__core"></span>
			</div>
			<div class="relative isolate mt-5">
				<div
					aria-hidden="true"
					class="pointer-events-none absolute inset-x-[-14%] top-1/2 h-16 -translate-y-1/2 rounded-full bg-[radial-gradient(circle,rgba(19,19,19,0.09)_0%,rgba(19,19,19,0)_74%)] blur-3xl"
				></div>
				<h1 class="nexal-wordmark relative text-[3.35rem] font-semibold tracking-[0.24em] sm:text-[4.4rem]">
					NEXAL
				</h1>
			</div>
		</div>
		<p class="nexal-subtitle text-muted-foreground mt-4 max-w-md text-center text-sm leading-relaxed">
			A multi-computer agent workspace for coordinating tasks, tools, and running workers.
		</p>
	</div>
</div>

<style>
	.nexal-lockup {
		animation: nexal-fade-up 0.9s cubic-bezier(0.16, 1, 0.3, 1) both;
	}

	.nexal-mark {
		position: relative;
		width: 5.5rem;
		height: 5.5rem;
		filter: drop-shadow(0 18px 34px rgba(15, 23, 42, 0.06));
		animation: nexal-drift-in 1s cubic-bezier(0.16, 1, 0.3, 1) both;
	}

	.nexal-mark__frame {
		position: absolute;
		inset: 0;
		border-radius: 1.6rem;
		border: 1px solid rgba(15, 23, 42, 0.08);
		background:
			linear-gradient(145deg, rgba(255, 255, 255, 0.98), rgba(244, 244, 245, 0.68));
		box-shadow:
			inset 0 1px 0 rgba(255, 255, 255, 0.9),
			0 10px 30px rgba(15, 23, 42, 0.03);
	}

	.nexal-mark__halo {
		position: absolute;
		inset: 0.42rem;
		border-radius: 1.25rem;
		border: 1px solid rgba(255, 255, 255, 0.65);
		background: linear-gradient(180deg, rgba(255, 255, 255, 0.18), rgba(255, 255, 255, 0));
	}

	.nexal-mark__rail,
	.nexal-mark__beam,
	.nexal-mark__core {
		position: absolute;
		display: block;
		border-radius: 999px;
		background: linear-gradient(180deg, rgba(17, 24, 39, 0.92), rgba(55, 65, 81, 0.76));
	}

	.nexal-mark__rail {
		top: 0.92rem;
		width: 0.44rem;
		height: 3.66rem;
	}

	.nexal-mark__rail--left {
		left: 1.32rem;
	}

	.nexal-mark__rail--right {
		right: 1.32rem;
	}

	.nexal-mark__beam {
		left: 1.7rem;
		top: 1.7rem;
		width: 2.28rem;
		height: 0.42rem;
		transform: rotate(34deg);
		transform-origin: left center;
	}

	.nexal-mark__beam--echo {
		top: 2.72rem;
		width: 1.78rem;
		opacity: 0.24;
	}

	.nexal-mark__core {
		left: 2.28rem;
		top: 2.18rem;
		width: 0.94rem;
		height: 0.94rem;
		background: linear-gradient(180deg, rgba(255, 255, 255, 0.98), rgba(226, 232, 240, 0.88));
		box-shadow:
			0 0 0 4px rgba(17, 24, 39, 0.03),
			0 0 20px rgba(255, 255, 255, 0.78);
	}

	.nexal-wordmark {
		animation: nexal-drift-in 1s cubic-bezier(0.16, 1, 0.3, 1) 0.08s both;
		background: linear-gradient(180deg, rgba(15, 23, 42, 0.98), rgba(71, 85, 105, 0.78));
		-webkit-background-clip: text;
		background-clip: text;
		color: transparent;
		text-shadow: 0 16px 30px rgba(15, 23, 42, 0.06);
	}

	.nexal-subtitle {
		animation: nexal-fade-up 0.9s cubic-bezier(0.16, 1, 0.3, 1) 0.18s both;
	}
</style>
