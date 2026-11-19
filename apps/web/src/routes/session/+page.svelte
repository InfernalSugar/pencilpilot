<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { currentSession, timeLabel } from '#lib/timed.js';
	import { gradable, gradeSubmission, recordGraded } from '#lib/grade-submission.js';
	import { imageUrl, titleOf } from '#lib/references.js';
	import { categories } from '#lib/categories.js';
	import type { Submission } from '#lib/submission.js';

	type Row = { sub: Submission; score: number | null; error?: boolean };

	let session = $state<ReturnType<typeof currentSession>>(null);
	let rows = $state<Row[]>([]);
	let graded = $state(0);

	onMount(async () => {
		session = currentSession();
		if (!session) return;
		rows = session.drawings.map((sub) => ({ sub, score: null }));
		// one at a time: grading is quick, and the page fills in as it goes
		for (const r of rows) {
			try {
				if (!gradable(r.sub)) throw new Error('no reference');
				const g = await gradeSubmission(r.sub);
				recordGraded(r.sub, g);
				r.score = r.sub.traced ? null : g.grade.score;
			} catch {
				r.error = true;
			}
			graded++;
		}
	});

	const scores = $derived(rows.map((r) => r.score).filter((s): s is number => s !== null));
	const average = $derived(
		scores.length ? scores.reduce((a, b) => a + b, 0) / scores.length : null
	);
	const best = $derived(scores.length ? Math.max(...scores) : null);
	const category = $derived(categories.find((c) => c.id === session?.plan.category));
	const againUrl = $derived(
		session
			? `/draw?category=${session.plan.category}&session=gesture&count=${session.plan.count}&time=${session.plan.time}` +
					(session.plan.topic ? `&topic=${session.plan.topic}` : '') +
					(session.plan.tier ? `&tier=${session.plan.tier}` : '')
			: '/'
	);
</script>

<svelte:head><title>Gesture session · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">
		Gesture session{category ? ` · ${category.name}` : ''}{session
			? ` · ${session.plan.count} × ${timeLabel(session.plan.time)}`
			: ''}
	</span>
	<h1>Session done</h1>

	{#if !session}
		<Tile tone="lilac">
			<h2>No session here</h2>
			<p>Start a gesture session from any category’s practice page.</p>
			<div><Button href="/">Pick a category</Button></div>
		</Tile>
	{:else}
		<div class="pp-bento stats">
			<Tile tone="ink">
				<span class="pp-eyebrow on-ink">Average</span>
				<span class="big">{average === null ? '–' : average.toFixed(1)}</span>
				<span class="note">
					{graded < rows.length ? `Grading ${graded + 1} of ${rows.length}…` : 'out of 10'}
				</span>
			</Tile>
			<Tile tone="yellow">
				<span class="pp-eyebrow ink">Best</span>
				<span class="big">{best === null ? '–' : best.toFixed(1)}</span>
			</Tile>
			<Tile>
				<span class="pp-eyebrow">Pictures</span>
				<span class="big">{rows.length}</span>
				<span class="note">
					{session.skipped ? `${session.skipped} ran out with nothing drawn` : 'all attempted'}
				</span>
			</Tile>
			<Tile tone="lilac">
				<span class="pp-eyebrow">Again?</span>
				<div class="actions">
					<Button href={againUrl} variant="dark">New session →</Button>
					<Button href="/progress" variant="outline">Progress</Button>
				</div>
			</Tile>
		</div>

		{#if rows.length}
			<ul class="grid" aria-label="Your drawings">
				{#each rows as r, i (r.sub.at)}
					{@const ref = r.sub.reference}
					<li>
						<a href="/results?at={r.sub.at}" class="card">
							<span class="pics">
								{#if ref}<img src={imageUrl(ref)} alt="Reference: {titleOf(ref)}" />{/if}
								<img class="drawing" src={r.sub.png} alt="Your drawing {i + 1}" />
							</span>
							<span class="meta">
								<span>{ref ? titleOf(ref) : `Drawing ${i + 1}`}</span>
								<b>
									{#if r.sub.traced}Traced{:else if r.error}–{:else if r.score === null}…{:else}{r.score.toFixed(
											1
										)}{/if}
								</b>
							</span>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</div>

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
		margin-bottom: var(--pp-gap);
	}
	.stats {
		margin-bottom: var(--pp-gap);
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
		opacity: 0.85;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: auto;
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
		gap: var(--pp-gap);
	}
	.card {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 16px;
		border-radius: 24px;
		background: var(--pp-surface);
		text-decoration: none;
		color: var(--pp-ink);
	}
	.card:hover {
		outline: 2px solid var(--pp-ink);
	}
	.pics {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 6px;
	}
	.pics img {
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
	}
	.pics .drawing {
		background: #fff;
		border: 1px solid var(--pp-track);
	}
	.pics .drawing:only-child {
		grid-column: 1 / -1;
	}
	.meta {
		display: flex;
		justify-content: space-between;
		gap: 8px;
		font-weight: 600;
	}
	.meta b {
		font: 800 22px var(--pp-font-display);
	}
</style>
