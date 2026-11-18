<script lang="ts">
	import type { Snippet } from 'svelte';

	type Tone = 'surface' | 'ink' | 'yellow' | 'lilac' | 'accent';

	let {
		tone = 'surface',
		span = 1,
		rows = 1,
		as = 'div',
		href,
		label,
		children
	}: {
		tone?: Tone;
		span?: number;
		rows?: number;
		as?: 'div' | 'section' | 'article';
		href?: string;
		label?: string;
		children: Snippet;
	} = $props();

	const style = $derived(`grid-column: span ${span}; grid-row: span ${rows};`);
</script>

{#if href}
	<a class="tile tone-{tone} link" {href} {style} aria-label={label}>{@render children()}</a>
{:else}
	<svelte:element this={as} class="tile tone-{tone}" {style} aria-label={label}>
		{@render children()}
	</svelte:element>
{/if}

<style>
	.tile {
		position: relative;
		overflow: hidden;
		padding: 28px;
		border-radius: var(--pp-radius-tile);
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-width: 0;
	}
	.tone-surface {
		background: var(--pp-surface);
	}
	.tone-ink {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.tone-yellow {
		background: var(--pp-yellow);
	}
	.tone-lilac {
		background: var(--pp-lilac);
	}
	.tone-accent {
		background: var(--pp-accent);
		color: #fff;
	}
	.link {
		text-decoration: none;
		color: inherit;
		transition: transform 0.25s var(--pp-spring);
	}
	.link:hover {
		transform: translateY(-4px) rotate(-0.6deg);
		color: inherit;
	}
</style>
