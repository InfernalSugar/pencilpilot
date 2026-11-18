<script lang="ts">
	import { onMount } from 'svelte';

	let { score }: { score: number } = $props();

	let shown = $state(0);
	let confetti = $state<{ x: number; d: number; r: number; c: string; s: number }[]>([]);
	const COLORS = ['#e8492f', '#ffd84d', '#d9d2ff', '#3d6bff', '#1f9d74', '#fff'];

	onMount(() => {
		const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
		if (reduce) {
			shown = score;
			return;
		}
		const start = performance.now();
		const dur = 900;
		let raf = 0;
		const tick = (t: number) => {
			const k = Math.min(1, (t - start) / dur);
			shown = score * (1 - Math.pow(1 - k, 3)); // ease-out
			if (k < 1) raf = requestAnimationFrame(tick);
			else if (score >= 8)
				confetti = Array.from({ length: 36 }, (_, i) => ({
					x: Math.random() * 100,
					d: Math.random() * 0.4,
					r: Math.random() * 360,
					c: COLORS[i % COLORS.length],
					s: 6 + Math.random() * 6
				}));
		};
		raf = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(raf);
	});
</script>

<span class="score" data-testid="score" aria-label="Score {score.toFixed(1)} out of 10">
	{shown.toFixed(1)}
</span>
{#each confetti as p, i (i)}
	<span
		class="bit"
		aria-hidden="true"
		style="left:{p.x}%;animation-delay:{p.d}s;background:{p.c};width:{p.s}px;height:{p.s *
			0.5}px;--r:{p.r}deg"
	></span>
{/each}

<style>
	.score {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 96px;
		line-height: 0.85;
		letter-spacing: -4px;
		font-variant-numeric: tabular-nums;
		margin-top: auto;
	}
	.bit {
		position: absolute;
		top: -10px;
		border-radius: 2px;
		pointer-events: none;
		animation: fall 1.6s cubic-bezier(0.2, 0.6, 0.4, 1) forwards;
		opacity: 0;
	}
	@keyframes fall {
		0% {
			opacity: 1;
			transform: translateY(0) rotate(var(--r));
		}
		100% {
			opacity: 0;
			transform: translateY(260px) rotate(calc(var(--r) + 540deg));
		}
	}
</style>
