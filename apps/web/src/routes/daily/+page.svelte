<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { dailyReference, dailyUrl, dayOf, msToNextDay, percentile } from '#lib/daily.js';
	import { imageUrl, tierName, tierOf, titleOf } from '#lib/references.js';
	import { categories } from '#lib/categories.js';

	const day = dayOf();
	const ref = dailyReference(day);
	const category = ref ? categories.find((c) => c.id === ref.category) : undefined;

	let stats = $state<{
		count: number;
		scores: number[];
		mine: number | null;
		streak: number;
	} | null>(null);
	let left = $state(msToNextDay());

	onMount(() => {
		fetch(`/api/daily?day=${day}`)
			.then((r) => (r.ok ? r.json() : null))
			.then((s) => (stats = s))
			.catch(() => {});
		const id = setInterval(() => (left = msToNextDay()), 1000);
		return () => clearInterval(id);
	});

	const countdown = $derived(
		`${Math.floor(left / 3_600_000)}h ${String(Math.floor((left / 60_000) % 60)).padStart(2, '0')}m`
	);
	const beat = $derived(
		stats?.mine != null ? Math.round(percentile(stats.mine, stats.scores) * 100) : null
	);
	const dateLabel = new Date(`${day}T12:00:00Z`).toLocaleDateString(undefined, {
		weekday: 'long',
		day: 'numeric',
		month: 'long'
	});
</script>

<svelte:head><title>Today’s challenge · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Daily challenge · {dateLabel}</span>
	<h1>Today’s challenge</h1>
	<p class="lede">One picture a day, the same for everyone. Your first try counts.</p>

	{#if !ref}
		<Tile tone="lilac">
			<h2>No challenge today</h2>
			<p>There are no pictures to pick from yet.</p>
		</Tile>
	{:else}
		<div class="pp-bento grid">
			<Tile span={2} rows={2} as="section" label="Today’s picture">
				<img class="ref" src={imageUrl(ref)} alt="Today’s picture: {titleOf(ref)}" />
				<h2>{titleOf(ref)}</h2>
				<p class="credit">
					{category?.name ?? ref.category} · {tierName(tierOf(ref))} · by
					<a href={ref.authorUrl} target="_blank" rel="noreferrer">{ref.author}</a> via
					<a href={ref.pageUrl} target="_blank" rel="noreferrer">Pixabay</a>
				</p>
			</Tile>
			<Tile tone="ink">
				{#if stats?.mine != null}
					<span class="pp-eyebrow on-ink">Your score today</span>
					<span class="big">{stats.mine.toFixed(1)}</span>
					<span class="note">
						{stats.count > 1
							? `Better than ${beat}% of ${stats.count - 1} other players`
							: 'First one in today!'}
					</span>
				{:else}
					<span class="pp-eyebrow on-ink">Players today</span>
					<span class="big">{stats?.count ?? '–'}</span>
					<span class="note">Next picture in {countdown}</span>
				{/if}
			</Tile>
			<Tile tone="yellow">
				<span class="pp-eyebrow ink">Daily streak</span>
				<span class="big">{stats?.streak ?? 0}</span>
				<span class="note">
					{page.data.user ? 'days in a row' : 'Sign in to keep a daily streak'}
				</span>
			</Tile>
			<Tile span={2} tone="lilac">
				{#if stats?.mine != null}
					<p class="plain">
						You’ve done today’s challenge. Another go is good practice, but only your first counts.
						Next picture in {countdown}.
					</p>
					<div class="row">
						<Button href={dailyUrl(day, ref)} variant="dark">Practise it again</Button>
					</div>
				{:else}
					<p class="plain">
						{page.data.user
							? 'Take your time: there’s no timer. Your score is compared with everyone else’s today.'
							: 'Anyone can play. Sign in to be counted and see how you compare.'}
					</p>
					<div class="row">
						<Button href={dailyUrl(day, ref)} variant="dark">Draw today’s picture →</Button>
						{#if !page.data.user}
							<Button href="/login?next=/daily" variant="outline">Sign in</Button>
						{/if}
					</div>
				{/if}
			</Tile>
		</div>
	{/if}
</div>

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
	}
	.lede {
		margin: 4px 0 var(--pp-gap);
		color: var(--pp-ink-2);
	}
	.ref {
		width: 100%;
		max-height: 420px;
		object-fit: contain;
		border-radius: 18px;
		background: var(--pp-surface-sunk);
	}
	h2 {
		font-size: 24px;
	}
	.credit {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 72px;
		line-height: 0.9;
		letter-spacing: -3px;
		margin-top: auto;
	}
	.note {
		font-size: 14px;
	}
	.plain {
		margin: 0;
		color: var(--pp-ink);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
</style>
