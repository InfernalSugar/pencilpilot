<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { loadSubmission, type Submission } from '#lib/submission.js';
	import { byId, imageUrl } from '#lib/references.js';

	let sub = $state<Submission | null>(null);
	let loaded = $state(false);
	const reference = $derived(byId(sub?.referenceId));
	onMount(() => {
		sub = loadSubmission();
		loaded = true;
	});
	const time = $derived(
		sub ? `${Math.floor(sub.seconds / 60)}m ${String(sub.seconds % 60).padStart(2, '0')}s` : ''
	);
</script>

<svelte:head><title>Results · PencilPilot</title></svelte:head>

<div class="pp-page">
	{#if loaded && !sub}
		<div class="pp-bento">
			<Tile span={4}>
				<h1>No drawing yet</h1>
				<p>Submit a drawing and it shows up here.</p>
				<div><Button href="/draw">Open the canvas</Button></div>
			</Tile>
		</div>
	{:else if sub}
		<div class="top">
			<div>
				<span class="pp-eyebrow">Results</span>
				<h1>Your drawing</h1>
			</div>
			<div class="actions">
				<Button href="/draw" variant="outline">Draw another</Button>
			</div>
		</div>
		<div class="pp-bento grid">
			<Tile tone="ink">
				<span class="pp-eyebrow on-ink">Your score</span>
				<span class="big">–</span>
				<span class="note">Scoring arrives with the grader (milestone M3).</span>
			</Tile>
			<Tile span={2} rows={2}>
				<div class="pair" class:single={!reference}>
					{#if reference}
						<figure>
							<img src={imageUrl(reference)} alt="The reference" />
							<figcaption>Reference</figcaption>
						</figure>
					{/if}
					<figure>
						<img src={sub.png} alt="Your submitted drawing" width={sub.width} height={sub.height} />
						<figcaption>Your drawing</figcaption>
					</figure>
				</div>
			</Tile>
			<Tile tone="yellow">
				<span class="pp-eyebrow ink">Time</span>
				<span class="big">{time}</span>
			</Tile>
			<Tile>
				<span class="pp-eyebrow">Strokes</span>
				<span class="big">{sub.strokes}</span>
				<span class="note">{sub.points.toLocaleString()} pen samples recorded</span>
			</Tile>
			<Tile tone="lilac">
				<span class="pp-eyebrow lilac">Brushes used</span>
				<span class="big">{sub.brushes.length}</span>
			</Tile>
		</div>
	{/if}
</div>

<style>
	.top {
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		align-items: center;
		gap: 16px;
		margin-bottom: 16px;
	}
	h1 {
		font-size: 30px;
	}
	.grid {
		grid-auto-rows: minmax(220px, auto);
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
		height: 100%;
	}
	.pair.single {
		grid-template-columns: 1fr;
	}
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-height: 0;
	}
	figcaption {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	img {
		min-height: 0;
		flex: 1;
		width: 100%;
		height: 100%;
		object-fit: contain;
		border-radius: 20px;
		background: #fff;
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 64px;
		line-height: 0.9;
		letter-spacing: -2px;
		margin-top: auto;
	}
	.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.ink {
		color: var(--pp-ink);
	}
	.lilac {
		color: var(--pp-lilac-ink);
	}
	.note {
		font-size: 14px;
		opacity: 0.85;
	}
	p {
		margin: 0;
		color: var(--pp-ink-2);
	}
</style>
