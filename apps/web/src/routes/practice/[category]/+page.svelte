<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import { TIERS, filtered, inCategory, tierOf } from '#lib/references.js';
	import { topicsFor } from '#lib/topics.js';
	import { recommendedTier } from '#lib/progress.js';
	import { liveAvailable } from '#lib/live.js';
	import { GESTURE_COUNTS, GESTURE_TIMES, TIME_LIMITS, timeLabel } from '#lib/timed.js';
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
	/** 'single' = one drawing (time 0 = no limit); 'gesture' = a timed queue of pictures */
	let timeMode = $state<'single' | 'gesture'>('single');
	let time = $state(0);
	let gestureCount = $state(10);
	let gestureTime = $state(60);
	const timing = $derived(
		timeMode === 'gesture'
			? `&session=gesture&count=${gestureCount}&time=${gestureTime}`
			: time
				? `&time=${time}`
				: ''
	);
	const drawUrl = (topic?: string) =>
		`/draw?category=${category.id}${topic ? `&topic=${topic}` : ''}&tier=${tier}${timing}`;
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

		<h2 class="section">2. Time</h2>
		<div class="time">
			<div class="chips" role="radiogroup" aria-label="Time limit">
				{#each TIME_LIMITS as t (t.seconds)}
					<button
						type="button"
						role="radio"
						aria-checked={timeMode === 'single' && time === t.seconds}
						onclick={() => {
							timeMode = 'single';
							time = t.seconds;
						}}
					>
						{t.label}
					</button>
				{/each}
				<button
					type="button"
					role="radio"
					aria-checked={timeMode === 'gesture'}
					onclick={() => (timeMode = 'gesture')}
				>
					Gesture session
				</button>
			</div>
			{#if timeMode === 'gesture'}
				<div class="gesture">
					<p>
						A run of pictures, a fixed time each, like a life-drawing class. Quick, loose, no
						stopping. You’ll see all your drawings and scores at the end.
					</p>
					<label>
						Pictures
						<select bind:value={gestureCount}>
							{#each GESTURE_COUNTS as n (n)}<option value={n}>{n}</option>{/each}
						</select>
					</label>
					<label>
						Each
						<select bind:value={gestureTime}>
							{#each GESTURE_TIMES as s (s)}<option value={s}>
									{TIME_LIMITS.find((t) => t.seconds === s)?.label}
								</option>{/each}
						</select>
					</label>
				</div>
			{:else if time}
				<p class="time-note">
					The canvas locks when time runs out and your drawing is scored straight away. Scores are
					compared with your other {timeLabel(time)} drawings.
				</p>
			{/if}
		</div>

		<h2 class="section">3. What to draw</h2>
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
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.chips button {
		min-height: 44px;
		padding: 8px 18px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 15px var(--pp-font-body);
		cursor: pointer;
	}
	.chips button[aria-checked='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.gesture {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 12px 20px;
		margin-top: 12px;
		padding: 18px 20px;
		border-radius: var(--pp-radius-card);
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
	}
	.gesture p {
		flex: 1 1 100%;
		margin: 0;
	}
	.gesture label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-weight: 600;
		font-size: 14px;
		color: var(--pp-ink);
	}
	.gesture select {
		min-height: 40px;
		padding: 4px 10px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 500 15px var(--pp-font-body);
	}
	.time-note {
		margin: 10px 0 0;
		color: var(--pp-ink-2);
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
