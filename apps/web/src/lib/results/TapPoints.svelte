<script lang="ts">
	import Button from '@pencilpilot/ui/Button.svelte';
	import { MIN_KEYPOINTS, refKeypoints, type Taps } from '../keypoints';
	import type { Reference } from '../references';

	let {
		reference,
		referenceSrc,
		drawingSrc,
		width,
		height,
		ondone,
		onskip
	}: {
		reference: Reference;
		referenceSrc: string;
		drawingSrc: string;
		/** the canvas size, in pixels */
		width: number;
		height: number;
		/** one entry per reference keypoint: where it was tapped (canvas pixels), or null */
		ondone: (taps: Taps) => void;
		onskip: () => void;
	} = $props();

	const kps = $derived(refKeypoints(reference));
	let taps = $state<Taps>([]);
	const current = $derived(taps.length);
	const finished = $derived(taps.length >= kps.length);
	const tapped = $derived(taps.filter(Boolean).length);

	function tap(e: MouseEvent) {
		// keyboard "clicks" have no position: the buttons below cover keyboard use
		if (finished || e.detail === 0) return;
		const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const fx = Math.min(1, Math.max(0, (e.clientX - b.left) / b.width));
		const fy = Math.min(1, Math.max(0, (e.clientY - b.top) / b.height));
		taps.push({ x: fx * width, y: fy * height });
	}

	const pct = (v: number) => `${(v * 100).toFixed(2)}%`;
</script>

<section class="tap" aria-label="Tap your points">
	<span class="pp-eyebrow">
		Proportions check · {Math.min(current + 1, kps.length)} of {kps.length}
	</span>
	<h2 aria-live="polite">
		{#if !finished}
			Tap the <b>{kps[current].label}</b> on your drawing
		{:else if tapped >= MIN_KEYPOINTS}
			All set!
		{:else}
			Too few points to check proportions
		{/if}
	</h2>
	<p class="lede">
		Find each numbered spot from the reference on your drawing and tap it there. It takes a few
		seconds and lets the grader check your proportions.
	</p>

	<div class="panes">
		<figure>
			<div
				class="frame"
				style="aspect-ratio: {reference.width} / {reference.height}; width: min(100%, calc(55vh * {reference.width /
					reference.height}))"
			>
				<img src={referenceSrc} alt="The reference" />
				{#each kps as k, i (i)}
					<span
						class="pt ref"
						class:now={i === current}
						class:done={i < current && taps[i]}
						class:missed={i < current && !taps[i]}
						style="left: {pct(k.x)}; top: {pct(k.y)}"
						title={k.label}
					>
						{i + 1}
					</span>
				{/each}
			</div>
			<figcaption>Reference</figcaption>
		</figure>
		<figure>
			<button
				type="button"
				class="frame area"
				style="aspect-ratio: {width} / {height}; width: min(100%, calc(55vh * {width / height}))"
				aria-label={finished ? 'Your drawing' : `Your drawing: tap the ${kps[current].label}`}
				onclick={tap}
				disabled={finished}
			>
				<img src={drawingSrc} alt="" />
				{#each taps as t, i (i)}
					{#if t}
						<span class="pt mine" style="left: {pct(t.x / width)}; top: {pct(t.y / height)}">
							{i + 1}
						</span>
					{/if}
				{/each}
			</button>
			<figcaption>Your drawing</figcaption>
		</figure>
	</div>

	<div class="actions">
		{#if finished}
			<Button variant="dark" onclick={() => ondone($state.snapshot(taps))}>See my score →</Button>
		{:else}
			<Button variant="outline" onclick={() => taps.push(null)}>I didn’t draw this one</Button>
		{/if}
		<Button variant="outline" onclick={() => taps.pop()} disabled={!taps.length}>Undo</Button>
		<button type="button" class="skip" onclick={onskip}>Skip the check</button>
	</div>
</section>

<style>
	.tap {
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		padding: 20px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	h2 {
		font-size: 26px;
		margin: 0;
		/* room for two lines, so the pictures don't jump as the label changes */
		min-height: 2.6em;
		line-height: 1.3;
	}
	h2 b {
		color: var(--pp-accent);
	}
	.lede {
		margin: 0;
		color: var(--pp-ink-2);
	}
	.panes {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 16px;
		margin-top: 8px;
	}
	@media (max-width: 640px) {
		.panes {
			grid-template-columns: 1fr;
		}
	}
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
		align-items: center;
	}
	figcaption {
		font-size: 13px;
		color: var(--pp-muted);
	}
	.frame {
		position: relative;
		background: #fff;
		border: 1px solid var(--pp-track);
		border-radius: 16px;
		overflow: hidden;
		padding: 0;
		display: block;
	}
	.frame img {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		pointer-events: none;
	}
	.area {
		cursor: crosshair;
		touch-action: manipulation;
	}
	.area:disabled {
		cursor: default;
	}
	.pt {
		position: absolute;
		width: 24px;
		height: 24px;
		margin: -12px 0 0 -12px;
		border-radius: 50%;
		font: 700 12px/20px var(--pp-font-body);
		text-align: center;
		border: 2px solid #fff;
		box-shadow: 0 1px 4px rgb(0 0 0 / 0.35);
		pointer-events: none;
	}
	.ref {
		background: #fff;
		color: var(--pp-ink);
		border-color: var(--pp-ink);
	}
	.ref.done {
		background: var(--pp-ink);
		color: #fff;
	}
	.ref.missed {
		opacity: 0.35;
	}
	.ref.now {
		background: var(--pp-accent);
		color: #fff;
		border-color: #fff;
		animation: pulse 1.2s ease-in-out infinite;
	}
	.mine {
		background: var(--pp-accent);
		color: #fff;
	}
	@keyframes pulse {
		50% {
			transform: scale(1.3);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ref.now {
			animation: none;
		}
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		align-items: center;
		margin-top: 8px;
	}
	.skip {
		margin-left: auto;
		background: none;
		border: 0;
		color: var(--pp-ink-2);
		text-decoration: underline;
		font: inherit;
		cursor: pointer;
	}
</style>
