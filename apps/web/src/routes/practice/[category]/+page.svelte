<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import { TIERS, filtered, inCategory, tierOf } from '#lib/references.js';
	import { topicsFor } from '#lib/topics.js';
	import { recommendedTier } from '#lib/progress.js';
	import { liveAvailable } from '#lib/live.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const category = $derived(data.category);
	const topics = $derived(topicsFor(category.id));
	const all = $derived(inCategory(category.id));

	let recommended = $state(1);
	/** Endless references available (ref-service up): every topic/tier is drawable. */
	let live = $state(false);
	let tier = $state(Number(page.url.searchParams.get('tier')) || 0);

	onMount(() => {
		liveAvailable().then((ok) => (live = ok));
		recommended = recommendedTier(category.id);
		if (!tier) {
			// Start at the recommended tier, or the nearest easier one that has pictures.
			const withRefs = TIERS.filter((t) => all.some((r) => tierOf(r) === t.id)).map((t) => t.id);
			tier = withRefs.filter((t) => t <= recommended).at(-1) ?? withRefs[0] ?? 1;
		}
	});

	const tierCount = (t: number) => all.filter((r) => tierOf(r) === t).length;
	const topicCount = (topic: string) => filtered(category.id, { topic, tier }).length;
	const anyCount = $derived(filtered(category.id, { tier }).length);
	const countLabel = (n: number) =>
		live
			? n
				? `Endless + ${n} hand-picked`
				: 'Endless'
			: n
				? `${n} at this level`
				: 'None at this level yet';
	const drawUrl = (topic?: string) =>
		`/draw?category=${category.id}${topic ? `&topic=${topic}` : ''}&tier=${tier}`;
</script>

<svelte:head><title>{category.name} · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="top">
		<span class="pp-eyebrow">Practice</span>
		<h1>{category.name}</h1>
		<p class="lede">Pick how hard you want it, then what to draw.</p>
	</div>

	{#if !all.length && !live}
		<div class="pp-bento">
			<Tile span={4} tone="lilac">
				<h2>No {category.name.toLowerCase()} references yet</h2>
				<p>We’re still picking pictures for this category. Try another one for now.</p>
			</Tile>
		</div>
	{:else}
		<h2 class="section">1. Difficulty</h2>
		<div class="tiers" role="radiogroup" aria-label="Difficulty">
			{#each TIERS as t (t.id)}
				<button
					type="button"
					role="radio"
					aria-checked={tier === t.id}
					class="tier"
					disabled={!live && !tierCount(t.id)}
					onclick={() => (tier = t.id)}
				>
					<span class="tier-top">
						<span class="dots" aria-hidden="true">
							{#each TIERS as d (d.id)}<span class:on={d.id <= t.id}></span>{/each}
						</span>
						{#if t.id === recommended}<span class="badge">Recommended</span>{/if}
					</span>
					<span class="tier-name">{t.name}</span>
					<span class="tier-blurb">{t.blurb}</span>
					<span class="count">{live ? 'Endless pictures' : `${tierCount(t.id)} pictures`}</span>
				</button>
			{/each}
		</div>

		<h2 class="section">2. What to draw</h2>
		<div class="topics">
			<a class="topic dark" href={drawUrl()} aria-disabled={!live && !anyCount}>
				<span class="topic-name">Surprise me</span>
				<span class="topic-blurb">Any {category.name.toLowerCase()} picture</span>
				<span class="count">{countLabel(anyCount)}</span>
			</a>
			{#each topics as t (t.id)}
				{@const n = topicCount(t.id)}
				<a class="topic" href={n || live ? drawUrl(t.id) : undefined} aria-disabled={!n && !live}>
					<span class="topic-name">{t.name}</span>
					<span class="topic-blurb">{t.blurb}</span>
					<span class="count">{countLabel(n)}</span>
				</a>
			{/each}
		</div>
	{/if}
</div>

<style>
	.top {
		margin-bottom: 8px;
	}
	h1 {
		font-size: clamp(40px, 6vw, 64px);
		line-height: 0.95;
		margin: 4px 0 8px;
	}
	.lede {
		margin: 0;
		font-size: 18px;
		color: var(--pp-ink-2);
	}
	.section {
		font-size: 22px;
		font-weight: 700;
		letter-spacing: -0.4px;
		margin: 32px 0 12px;
	}
	.tiers {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
		gap: var(--pp-gap);
	}
	.tier {
		text-align: left;
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 22px;
		border: 3px solid transparent;
		border-radius: var(--pp-radius-card);
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: inherit;
		cursor: pointer;
		transition: transform 0.2s var(--pp-spring);
	}
	.tier:hover:not(:disabled) {
		transform: translateY(-3px);
	}
	.tier[aria-checked='true'] {
		border-color: var(--pp-accent);
	}
	.tier:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.tier-top {
		display: flex;
		justify-content: space-between;
		align-items: center;
		min-height: 26px;
	}
	.dots {
		display: flex;
		gap: 4px;
	}
	.dots span {
		width: 10px;
		height: 10px;
		border-radius: 999px;
		background: var(--pp-track);
	}
	.dots span.on {
		background: var(--pp-accent);
	}
	.badge {
		padding: 4px 10px;
		border-radius: 999px;
		background: var(--pp-yellow);
		font-size: 12px;
		font-weight: 700;
	}
	.tier-name {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 28px;
		letter-spacing: -0.8px;
	}
	.tier-blurb {
		font-size: 14px;
		color: var(--pp-ink-2);
	}
	.count {
		margin-top: auto;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.topics {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: var(--pp-gap);
	}
	.topic {
		min-height: 150px;
		padding: 22px;
		display: flex;
		flex-direction: column;
		gap: 6px;
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		text-decoration: none;
		transition: transform 0.25s var(--pp-spring);
	}
	.topic:hover {
		transform: translateY(-4px) rotate(-0.6deg);
		color: inherit;
	}
	.topic[aria-disabled='true'] {
		opacity: 0.5;
		pointer-events: none;
	}
	.topic.dark {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.topic.dark .topic-blurb,
	.topic.dark .count {
		color: var(--pp-on-ink-muted);
	}
	.topic-name {
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 24px;
	}
	.topic-blurb {
		font-size: 14px;
		color: var(--pp-ink-2);
	}
	p {
		margin: 0;
	}
</style>
