<script lang="ts">
	import { onMount } from 'svelte';
	import { warmGrader } from '../grader';
	import { page } from '$app/state';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import PaintCanvas, {
		type CanvasStatus,
		BLANK_STATUS
	} from '@pencilpilot/paint/PaintCanvas.svelte';
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail from '@pencilpilot/paint/ToolRail.svelte';
	import {
		DRILL_BRUSHES,
		SKILL_NAMES,
		saveDrillResult,
		seeded,
		type Drill,
		type DrillBrush,
		type DrillTarget
	} from '#lib/drills.js';
	import { gradeRound, type RoundResult } from '#lib/drill-grade.js';
	import { imageUrl, pickReference, type Reference } from '#lib/references.js';
	import { fetchLiveReference, recentRefs, rememberRef } from '#lib/live.js';
	import RefCredit from '#lib/RefCredit.svelte';

	let { drill }: { drill: Drill } = $props();

	const W = 800;
	const H = 600;
	const BLANK: CanvasStatus = BLANK_STATUS;

	type Phase = 'drawing' | 'grading' | 'revealed' | 'done';

	let seed = $state(0);
	let round = $state(0);
	let target = $state<DrillTarget>();
	let reference = $state<Reference | null>(null);
	let loading = $state(true);
	let phase = $state<Phase>('drawing');
	let results = $state<RoundResult[]>([]);
	let elapsed = $state(0);
	let status = $state<CanvasStatus>(BLANK);
	let canvasReady = $state(false);
	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	let brush = $state<DrillBrush>('pen');
	let eraser = $state(false);
	let size = $state(1);
	let gradeError = $state('');

	const brushId = $derived(DRILL_BRUSHES.find((b) => b.id === brush)!.brushId);
	const isLast = $derived(round + 1 >= drill.rounds);
	const countdown = $derived(drill.secondsPerRound ? drill.secondsPerRound - elapsed : null);
	const clockSeconds = $derived(Math.max(0, countdown ?? elapsed));
	const clock = $derived(
		`${String(Math.floor(clockSeconds / 60)).padStart(2, '0')}:${String(clockSeconds % 60).padStart(2, '0')}`
	);
	const scores = $derived(results.map((r) => r.score));
	const avg = $derived(
		scores.length ? Math.round((scores.reduce((s, x) => s + x, 0) / scores.length) * 10) / 10 : 0
	);
	const best = $derived(scores.length ? scores.indexOf(Math.max(...scores)) : -1);
	const lineConfidence = $derived.by(() => {
		const v = results.map((r) => r.lineConfidence).filter((x): x is number => x !== null);
		return v.length ? Math.round((v.reduce((s, x) => s + x, 0) / v.length) * 100) : null;
	});
	const hideInk = $derived(!!drill.blind && phase !== 'revealed');

	const primaryLabel = $derived.by(() => {
		if (phase === 'grading') return 'Scoring…';
		if (phase === 'revealed') return isLast ? 'See results →' : 'Next round →';
		if (drill.blind) return 'Finish & reveal';
		if (isLast) return 'Finish drill →';
		return status.blank ? 'Skip round →' : 'Next round →';
	});

	/** Contour drills: a real picture (Endless, then curated), else a generated subject. */
	async function loadSubject(): Promise<Reference | null> {
		const category = Math.random() < 0.5 ? 'animals' : 'real-life';
		const avoid = recentRefs();
		let r =
			(await fetchLiveReference({ category, tier: 1, exclude: avoid })) ??
			pickReference(category, avoid.at(-1), Math.random, { tier: 1 }) ??
			pickReference(category, avoid.at(-1));
		if (!r) return null;
		try {
			const img = new Image();
			img.src = imageUrl(r);
			await img.decode();
		} catch {
			r = undefined; // picture not downloaded / service gone: use the generated subject
		}
		if (r) rememberRef(r.id);
		return r ?? null;
	}

	async function setupRound() {
		loading = true;
		phase = 'drawing';
		elapsed = 0;
		status = BLANK;
		canvasReady = false;
		eraser = false;
		gradeError = '';
		const rng = seeded(seed + round * 7919);
		const t = drill.target(rng, W, H);
		reference = drill.kind === 'reference' ? await loadSubject() : null;
		target = t;
		loading = false;
	}

	async function endRound() {
		if (phase !== 'drawing' || loading || !target) return;
		phase = 'grading';
		let r: RoundResult;
		try {
			const d = canvas?.exportDrawing();
			r = d
				? await gradeRound(
						drill,
						d,
						$state.snapshot(target) as DrillTarget,
						reference ? ($state.snapshot(reference) as Reference) : null
					)
				: { score: 1, graderScore: 1, lineConfidence: null, line: null };
		} catch (e) {
			gradeError = `Could not score that round: ${e}`;
			r = { score: 1, graderScore: 1, lineConfidence: null, line: null };
		}
		results = [...results, r];
		if (drill.blind) phase = 'revealed';
		else advance();
	}

	function advance() {
		if (results.length >= drill.rounds) return finish();
		round++;
		setupRound();
	}

	function finish() {
		phase = 'done';
		saveDrillResult({ id: drill.id, at: Date.now(), avg, rounds: [...scores] });
	}

	function primary() {
		if (phase === 'drawing') endRound();
		else if (phase === 'revealed') advance();
	}

	function again() {
		seed = (seed + 104729) >>> 0;
		round = 0;
		results = [];
		setupRound();
	}

	onMount(() => {
		warmGrader();
		brush = drill.brush;
		const fromUrl = Number(page.url.searchParams.get('seed'));
		seed = Number.isFinite(fromUrl) && fromUrl > 0 ? fromUrl : Date.now() % 2147483647;
		setupRound();
		const id = setInterval(() => {
			if (phase !== 'drawing' || loading || !canvasReady) return;
			elapsed++;
			if (countdown !== null && countdown <= 0) endRound();
		}, 1000);
		return () => clearInterval(id);
	});

	function onkey(e: KeyboardEvent) {
		if ((e.target as HTMLElement).closest('input, textarea, select')) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod && k === 'z') {
			e.preventDefault();
			if (e.shiftKey) canvas?.redo();
			else canvas?.undo();
		} else if (!mod && k === 'b') eraser = false;
		else if (!mod && k === 'e') eraser = true;
	}

	const fmt = (v: number) => v.toFixed(1);
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>{drill.name} · Drills · PencilPilot</title></svelte:head>

{#if phase === 'done'}
	<div class="pp-page">
		<div class="top">
			<div>
				<a class="pp-eyebrow back" href="/drills">Drills · {SKILL_NAMES[drill.skill]}</a>
				<h1>{drill.name}</h1>
			</div>
		</div>
		<section class="summary pp-bento" aria-label="Drill summary">
			<Tile tone="ink" span={2} rows={2} label="Average score">
				<span class="pp-eyebrow on-ink">Average score</span>
				<p class="big" data-testid="drill-average">{fmt(avg)}<small>/10</small></p>
				<p class="on-ink-muted">
					{drill.rounds} rounds of {drill.name.toLowerCase()}. Saved to your progress.
				</p>
			</Tile>
			<Tile span={2} label="Rounds">
				<h2>Rounds</h2>
				<ol class="bars">
					{#each scores as s, i (i)}
						<li class:best={i === best}>
							<span class="bar" style="height: {s * 10}%"></span>
							<span class="val">{fmt(s)}</span>
							<span class="num">{i + 1}</span>
						</li>
					{/each}
				</ol>
			</Tile>
			<Tile tone="yellow" label="Best round">
				<span class="pp-eyebrow dark">Best round</span>
				<p class="mid">#{best + 1} · {fmt(scores[best] ?? 0)}</p>
			</Tile>
			<Tile tone="lilac" label="Line confidence">
				<span class="pp-eyebrow dark">Line confidence</span>
				<p class="mid">{lineConfidence === null ? '–' : `${lineConfidence}%`}</p>
			</Tile>
			<Tile span={4} label="What next">
				<div class="actions">
					<Button onclick={again}>Again ↻</Button>
					<Button variant="outline" href="/lessons">Back to lessons</Button>
					<Button variant="soft" href="/drills">All drills</Button>
				</div>
			</Tile>
		</section>
	</div>
{:else}
	<Studio>
		{#snippet title()}
			<a class="pp-eyebrow back" href="/drills">Drills · {SKILL_NAMES[drill.skill]}</a>
			<h1>{drill.name}</h1>
		{/snippet}
		{#snippet actions()}
			<span class="chip round" aria-label="Round {round + 1} of {drill.rounds}">
				Round <b>{round + 1} / {drill.rounds}</b>
			</span>
			<span
				class="chip timer"
				class:urgent={countdown !== null && countdown <= 10}
				aria-label={countdown !== null ? 'Time left' : 'Time spent'}
			>
				<svg
					width="20"
					height="20"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.4"
					stroke-linecap="round"
					aria-hidden="true"
				>
					<circle cx="12" cy="13" r="8" /><path d="M12 9v4l2.5 2M9 2h6" />
				</svg>
				{clock}
			</span>
			<Button onclick={primary} disabled={phase === 'grading' || loading}>{primaryLabel}</Button>
		{/snippet}
		{#snippet rail()}
			<ToolRail tools={['brush', 'eraser']} bind:eraser bind:size showOpacity={false} />
		{/snippet}
		{#snippet options()}
			<div class="brushes" role="radiogroup" aria-label="Brush">
				{#each DRILL_BRUSHES as b (b.id)}
					<button
						type="button"
						class="opt"
						role="radio"
						aria-checked={!eraser && brush === b.id}
						onclick={() => {
							brush = b.id;
							eraser = false;
						}}
					>
						{b.name}
					</button>
				{/each}
			</div>
			<button
				type="button"
				class="opt"
				aria-label="Undo (Ctrl+Z)"
				title="Undo (Ctrl+Z)"
				disabled={!status.canUndo}
				onclick={() => canvas?.undo()}
			>
				Undo
			</button>
			<button
				type="button"
				class="opt"
				aria-label="Clear canvas"
				title="Clear canvas"
				disabled={status.blank}
				onclick={() => canvas?.clear()}
			>
				Clear
			</button>
		{/snippet}
		{#snippet stage()}
			<div class="fit">
				<div class="canvas-tile" class:locked={phase !== 'drawing'}>
					{#key `${seed}-${round}`}
						<PaintCanvas
							bind:this={canvas}
							{brushId}
							{eraser}
							{size}
							viewControls={false}
							onchange={(s) => {
								status = s;
								canvasReady = true;
							}}
						/>
					{/key}
					{#if target?.guide && !loading}
						<div class="overlay guide" data-guide aria-hidden="true">
							<!-- eslint-disable-next-line svelte/no-at-html-tags -- generated by drills.ts, no user input -->
							{@html target.guide}
						</div>
					{/if}
					{#if hideInk}
						<div class="overlay blind" aria-hidden="true">
							<span>Drawing blind. Keep your eyes on the picture.</span>
						</div>
					{/if}
					{#if phase === 'revealed'}
						<p class="reveal" role="status">
							Here’s your blind contour. Line confidence: {fmt(results.at(-1)?.score ?? 1)}/10
						</p>
					{/if}
				</div>
			</div>
		{/snippet}
		{#snippet dockTop()}
			<Tile as="section" label="Target">
				<div class="tile-head">
					<h2>{drill.kind === 'reference' ? 'Picture' : 'Target'}</h2>
					{#if drill.blind}<span class="badge">Looking exercise</span>{/if}
					{#if drill.creative}<span class="badge">Creative exercise</span>{/if}
				</div>
				{#if loading || !target}
					<div class="target skeleton" aria-busy="true">Getting the next one…</div>
				{:else if reference}
					<img
						class="target"
						src={imageUrl(reference)}
						alt="Subject: {reference.tags.join(', ')}"
					/>
					<p class="credit">
						<RefCredit {reference} />
					</p>
				{:else}
					<div class="target svg" data-testid="drill-target">
						<!-- eslint-disable-next-line svelte/no-at-html-tags -- generated by drills.ts, no user input -->
						{@html target.prompt}
					</div>
				{/if}
			</Tile>
			<Tile as="section" label="How to">
				<h2>How to</h2>
				<ol class="howto">
					{#each drill.howTo as step, i (i)}<li>{step}</li>{/each}
				</ol>
				{#if results.length}
					<p class="so-far" aria-label="Scores so far">
						So far:
						{#each scores as s, i (i)}<span class="pill">{fmt(s)}</span>{/each}
					</p>
				{/if}
				{#if gradeError}<p class="error" role="alert">{gradeError}</p>{/if}
			</Tile>
		{/snippet}
	</Studio>
{/if}

<style>
	/* the drill canvas keeps its 4:3 shape so the guide overlay lines up with it */
	.fit {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		container-type: size;
	}
	.fit .canvas-tile {
		width: min(100cqw, calc(100cqh * 4 / 3));
		aspect-ratio: 4 / 3;
	}
	.brushes {
		display: flex;
		gap: 6px;
	}
	.opt {
		height: 38px;
		padding: 0 12px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.opt[aria-checked='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.opt:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.top {
		display: flex;
		flex-wrap: wrap;
		gap: 16px;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 16px;
	}
	.back {
		text-decoration: none;
	}
	h1 {
		font-size: 30px;
		letter-spacing: -0.8px;
	}
	h2 {
		font-size: 20px;
		font-weight: 700;
		letter-spacing: -0.3px;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		min-height: 48px;
		padding: 10px 18px;
		border-radius: var(--pp-radius-btn);
		font: 600 16px var(--pp-font-body);
	}
	.chip b {
		font: 800 22px var(--pp-font-display);
	}
	.round {
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
	}
	.timer {
		background: var(--pp-yellow);
		font: 700 22px var(--pp-font-display);
		font-variant-numeric: tabular-nums;
	}
	.timer.urgent {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	@media (max-width: 960px) {
		/* Phone/tablet: target, then canvas, then the how-to. (.layout raises specificity over the
		   base `.side` rule further down.) */
	}
	.tile-head {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
	}
	.badge {
		padding: 2px 10px;
		border-radius: 999px;
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font-size: 12px;
		font-weight: 600;
	}
	.target {
		display: block;
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		border-radius: 18px;
		background: #fff;
		border: 2px solid var(--pp-track);
		overflow: hidden;
	}
	.target.svg :global(svg) {
		display: block;
		width: 100%;
		height: 100%;
	}
	.skeleton {
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
		background: var(--pp-surface-sunk);
	}
	.credit {
		margin: 0;
		font-size: 12px;
		color: var(--pp-muted);
	}
	.howto {
		margin: 0;
		padding-left: 22px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		color: var(--pp-ink-2);
		line-height: 1.45;
	}
	.so-far {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		align-items: center;
		margin: 4px 0 0;
		font-size: 14px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.pill {
		padding: 2px 10px;
		border-radius: 999px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font-variant-numeric: tabular-nums;
	}
	.error {
		margin: 0;
		color: var(--pp-accent-ink);
		font-size: 14px;
	}
	.canvas-tile {
		position: relative;
		background: #fff;
		border-radius: var(--pp-radius-tile);
		overflow: hidden;
		box-shadow: 0 1px 0 rgba(22, 22, 26, 0.04);
	}
	.canvas-tile.locked {
		pointer-events: none;
	}
	.overlay {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	.guide {
		opacity: 0.75;
	}
	.guide :global(svg) {
		display: block;
		width: 100%;
		height: 100%;
	}
	.blind {
		background: #fff;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
		text-align: center;
		padding: 24px;
	}
	.reveal {
		position: absolute;
		left: 16px;
		bottom: 16px;
		margin: 0;
		padding: 8px 14px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font-weight: 600;
		font-size: 14px;
	}

	/* summary */
	.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.on-ink-muted {
		margin: 0;
		color: var(--pp-on-ink-muted);
	}
	.dark {
		color: var(--pp-ink-2);
	}
	.big {
		margin: auto 0 0;
		font: 800 clamp(72px, 14vw, 128px) / 0.9 var(--pp-font-display);
		letter-spacing: -0.04em;
		color: var(--pp-yellow);
	}
	.big small {
		font-size: 0.3em;
		color: var(--pp-on-ink-muted);
		margin-left: 6px;
	}
	.mid {
		margin: 0;
		font: 800 40px var(--pp-font-display);
		letter-spacing: -0.03em;
	}
	.bars {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		align-items: flex-end;
		gap: 10px;
		height: 160px;
	}
	.bars li {
		flex: 1;
		height: 100%;
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		align-items: center;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.bar {
		width: 100%;
		max-width: 48px;
		min-height: 6px;
		border-radius: 10px 10px 4px 4px;
		background: var(--pp-lilac);
		order: 1;
	}
	.bars .val {
		order: 0;
		color: var(--pp-ink);
		font-variant-numeric: tabular-nums;
	}
	.bars .num {
		order: 2;
	}
	.bars li.best .bar {
		background: var(--pp-accent);
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
	}
</style>
