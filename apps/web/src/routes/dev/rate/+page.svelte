<script lang="ts">
	import { enhance } from '$app/forms';
	import type { PageProps } from './$types';
	let { data }: PageProps = $props();
	const s = $derived(data.todo[0]);
</script>

<svelte:head><title>Rate drawings · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Dev only · M8-T01 calibration</span>
	<h1>Rate drawings</h1>
	<p class="lede">
		How good is this copy, as an art teacher would mark it? Ignore the computer’s score. Labels so
		far:
		{#each Object.entries(data.byCat) as [c, n] (c)}<b>{c}</b> {n} ·
		{/each}
		{data.todo.length} of {data.total} left. Aim for 20–30 per category, then run
		<code>cargo run --release -p calibrate -- fit</code>.
	</p>
	{#if !s}
		<p>
			Nothing to rate. On the results page (dev server), press <b>Save for calibration</b> after grading
			a drawing.
		</p>
	{:else}
		<div class="pair">
			<figure>
				<img src={s.refUrl} alt="Reference" />
				<figcaption>Reference ({s.category})</figcaption>
			</figure>
			<figure>
				<img src="/dev/calibration?img={s.id}" alt="Drawing" />
				<figcaption>
					Drawing (grader said {s.score.toFixed(1)}; hidden from your judgement please)
				</figcaption>
			</figure>
		</div>
		<form method="POST" use:enhance class="scale" aria-label="Your rating">
			<input type="hidden" name="sample" value={s.id} />
			{#each [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as n (n)}
				<button name="human" value={n}>{n}</button>
			{/each}
		</form>
	{/if}
</div>

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 8px;
	}
	.lede {
		color: var(--pp-ink-2);
		max-width: 900px;
		line-height: 1.6;
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 16px;
	}
	figure {
		margin: 0;
	}
	img {
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		background: #fff;
		border-radius: 20px;
	}
	figcaption {
		font-size: 13px;
		color: var(--pp-muted);
	}
	.scale {
		display: flex;
		gap: 8px;
		margin-top: 16px;
		flex-wrap: wrap;
	}
	.scale button {
		width: 56px;
		height: 56px;
		border: 0;
		border-radius: 16px;
		background: var(--pp-surface);
		font: 800 20px var(--pp-font-display);
		cursor: pointer;
	}
	.scale button:hover {
		background: var(--pp-yellow);
	}
</style>
