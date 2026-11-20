<script lang="ts" module>
	export interface QuickAction {
		id: string;
		label: string;
		/** an SVG path (24×24) */
		icon: string;
		run: () => void;
		disabled?: boolean;
	}
</script>

<script lang="ts">
	/**
	 * Procreate's QuickMenu: a ring of everyday actions where you right-click
	 * (or long-press) the canvas. Escape, or a click outside, closes it.
	 */
	import { onMount } from 'svelte';

	let {
		x,
		y,
		actions,
		onclose
	}: {
		x: number;
		y: number;
		actions: QuickAction[];
		onclose: () => void;
	} = $props();

	const R = 74;
	// keep the ring on screen
	const cx = $derived(Math.min(window.innerWidth - R - 34, Math.max(R + 34, x)));
	const cy = $derived(Math.min(window.innerHeight - R - 34, Math.max(R + 34, y)));
	let ring = $state<HTMLDivElement>();

	onMount(() => {
		ring?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
		const key = (e: KeyboardEvent) => e.key === 'Escape' && onclose();
		window.addEventListener('keydown', key);
		return () => window.removeEventListener('keydown', key);
	});
</script>

<div
	class="scrim"
	role="presentation"
	onpointerdown={onclose}
	oncontextmenu={(e) => {
		e.preventDefault();
		onclose();
	}}
></div>
<div
	bind:this={ring}
	class="ring"
	role="menu"
	aria-label="Quick menu"
	style="left: {cx}px; top: {cy}px"
>
	{#each actions as a, i (a.id)}
		{@const ang = (i / actions.length) * Math.PI * 2 - Math.PI / 2}
		<button
			type="button"
			role="menuitem"
			class="item"
			style="transform: translate({Math.cos(ang) * R}px, {Math.sin(ang) * R}px)"
			disabled={a.disabled}
			aria-label={a.label}
			title={a.label}
			onclick={() => {
				a.run();
				onclose();
			}}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d={a.icon} /></svg>
		</button>
	{/each}
	<span class="hub" aria-hidden="true"></span>
</div>

<style>
	.scrim {
		position: fixed;
		inset: 0;
		z-index: 60;
	}
	.ring {
		position: fixed;
		z-index: 61;
		width: 1px;
		height: 1px;
	}
	.item {
		position: absolute;
		left: -26px;
		top: -26px;
		display: grid;
		place-items: center;
		width: 52px;
		height: 52px;
		border: 0;
		border-radius: 50%;
		background: #23232b;
		color: #fff;
		box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);
		cursor: pointer;
		animation: pop 0.14s ease-out both;
	}
	.item:hover,
	.item:focus-visible {
		background: var(--pp-accent, #2f6bff);
		outline: none;
	}
	.item:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.item svg {
		width: 24px;
		height: 24px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.hub {
		position: absolute;
		left: -6px;
		top: -6px;
		width: 12px;
		height: 12px;
		border-radius: 50%;
		background: #23232b;
		opacity: 0.6;
	}
	@keyframes pop {
		from {
			opacity: 0;
			scale: 0.6;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.item {
			animation: none;
		}
	}
</style>
