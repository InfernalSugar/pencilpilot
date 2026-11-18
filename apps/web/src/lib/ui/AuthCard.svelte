<script lang="ts">
	import type { Snippet } from 'svelte';
	let {
		title,
		eyebrow,
		children,
		aside
	}: { title: string; eyebrow: string; children: Snippet; aside?: Snippet } = $props();
</script>

<div class="pp-page">
	<div class="wrap">
		<section class="card" aria-label={title}>
			<span class="pp-eyebrow">{eyebrow}</span>
			<h1>{title}</h1>
			{@render children()}
		</section>
		{#if aside}<aside class="aside">{@render aside()}</aside>{/if}
	</div>
</div>

<style>
	.wrap {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
		gap: var(--pp-gap);
		max-width: 980px;
		margin: 16px auto 0;
	}
	@media (max-width: 800px) {
		.wrap {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.card,
	.aside {
		padding: 36px;
		border-radius: var(--pp-radius-tile);
		display: flex;
		flex-direction: column;
		gap: 16px;
	}
	.card {
		background: var(--pp-surface);
	}
	.aside {
		background: var(--pp-lilac);
	}
	h1 {
		font-size: clamp(36px, 5vw, 52px);
		line-height: 0.95;
	}
	.card :global(form) {
		display: flex;
		flex-direction: column;
		gap: 14px;
	}
	.card :global(label) {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-weight: 600;
		font-size: 14px;
	}
	.card :global(input) {
		padding: 14px 16px;
		border: 2px solid var(--pp-track);
		border-radius: 14px;
		font: 500 16px var(--pp-font-body);
		background: var(--pp-ground);
	}
	.card :global(input:focus) {
		outline: none;
		border-color: var(--pp-ink);
	}
	.card :global(.error) {
		margin: 0;
		padding: 12px 14px;
		border-radius: 12px;
		background: #fde4df;
		color: var(--pp-accent-ink);
		font-weight: 600;
		font-size: 14px;
	}
	.card :global(.or) {
		display: flex;
		align-items: center;
		gap: 10px;
		color: var(--pp-muted);
		font-size: 13px;
	}
	.card :global(.or::before),
	.card :global(.or::after) {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--pp-track);
	}
	.card :global(.google) {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 10px;
		min-height: 48px;
		border-radius: 16px;
		border: 2px solid var(--pp-track);
		background: #fff;
		font: 600 16px var(--pp-font-body);
		text-decoration: none;
		color: var(--pp-ink);
	}
	.card :global(.google:hover) {
		border-color: var(--pp-ink);
	}
	.card :global(.small) {
		margin: 0;
		font-size: 14px;
		color: var(--pp-muted);
	}
</style>
