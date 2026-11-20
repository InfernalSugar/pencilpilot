<script lang="ts">
	import { page } from '$app/state';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import { lessonForRule, skillLabel } from '#lib/lessons.js';
	import { ruleById } from '#lib/rules.js';
	import { attempts, recurringMistakes, type Pattern } from '#lib/progress.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const lesson = $derived(data.lesson);
	const steps = $derived(lesson.steps);

	let step = $state(0);
	let showAll = $state(false);
	// Start at the first step whenever the lesson changes (e.g. via "Up next").
	$effect.pre(() => {
		void lesson.id;
		step = 0;
	});

	const current = $derived(steps[step]);
	/** The diagram for this step: its own, else the latest one before it, else the first. */
	const diagram = $derived(
		steps
			.slice(0, step + 1)
			.flatMap((s) => s.diagrams)
			.at(-1) ?? steps.flatMap((s) => s.diagrams)[0]
	);
	const progress = $derived(showAll ? 1 : (step + 1) / steps.length);

	// "Why you're here": only when linked from feedback with ?from=<ruleId>.
	const fromId = $derived(page.url.searchParams.get('from'));
	const from = $derived(
		fromId && lessonForRule(fromId)?.id === lesson.id ? ruleById(fromId) : undefined
	);
	let pattern = $state<Pattern | undefined>();
	$effect(() => {
		const id = from?.id;
		pattern = id ? recurringMistakes(attempts(), 5, 2).find((p) => p.ruleId === id) : undefined;
	});

	const go = (d: number) => (step = Math.min(steps.length - 1, Math.max(0, step + d)));

	function onkeydown(e: KeyboardEvent) {
		if (showAll || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
		const t = e.target as HTMLElement | null;
		if (t?.closest('input, textarea, select, [contenteditable]')) return;
		if (e.key === 'ArrowRight') go(1);
		else if (e.key === 'ArrowLeft') go(-1);
		else return;
		e.preventDefault();
	}
</script>

<svelte:window {onkeydown} />
<svelte:head><title>{lesson.title} · Lessons · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="board" class:has-why={!!from} class:all={showAll}>
		<Tile area="title" as="section">
			<span class="pp-eyebrow">
				<a href="/lessons">Lessons</a> · {skillLabel(lesson.skill)} · {lesson.minutes} min read
			</span>
			<h1>{lesson.title}</h1>
			<p class="lede">{lesson.summary}</p>
			<div class="counter">
				<span>{showAll ? `All ${steps.length} steps` : `Step ${step + 1} of ${steps.length}`}</span>
				<div
					class="bar"
					role="progressbar"
					aria-label="Lesson progress"
					aria-valuemin={0}
					aria-valuemax={steps.length}
					aria-valuenow={showAll ? steps.length : step + 1}
				>
					<span style="width: {progress * 100}%"></span>
				</div>
			</div>
		</Tile>

		{#if from}
			<Tile area="why" tone="ink" label="Why you’re here">
				<span class="pp-eyebrow on-ink">Why you’re here</span>
				<p class="why">{from.title}</p>
				{#if pattern}
					<p class="why-count">
						In {pattern.count} of your last {pattern.window} drawings. This lesson is the quickest fix.
					</p>
				{:else}
					<p class="why-count">It came up in your last drawing. Three minutes here will help.</p>
				{/if}
			</Tile>
		{/if}

		<Tile area="learn" tone="yellow">
			<span class="pp-eyebrow ink">You’ll learn</span>
			<ul class="learn">
				{#each lesson.learn as item (item)}<li>{item}</li>{/each}
			</ul>
		</Tile>

		{#if showAll}
			<Tile area="step" as="article" label="All steps">
				<div class="step-head">
					<h2 class="visually-small">All steps</h2>
					<Button variant="soft" aria-pressed={true} onclick={() => (showAll = false)}>
						One step at a time
					</Button>
				</div>
				{#each steps as s, i (i)}
					<section class="all-step">
						<span class="pp-eyebrow">Step {i + 1}</span>
						<h2>{s.title}</h2>
						<!-- eslint-disable-next-line svelte/no-at-html-tags -- our own sanitised lesson content -->
						<div class="prose">{@html s.html}</div>
						{#each s.diagrams as d, j (j)}
							<!-- eslint-disable-next-line svelte/no-at-html-tags -- our own hand-written SVG -->
							<div class="diagram inline">{@html d}</div>
						{/each}
					</section>
				{/each}
			</Tile>
		{:else}
			{#if diagram}
				<Tile area="diagram" label="Diagram">
					<!-- eslint-disable-next-line svelte/no-at-html-tags -- our own hand-written SVG -->
					<div class="diagram">{@html diagram}</div>
				</Tile>
			{/if}
			<Tile area="step" as="article">
				<span class="pp-eyebrow">Step {step + 1}</span>
				<h2>{current.title}</h2>
				<!-- eslint-disable-next-line svelte/no-at-html-tags -- our own sanitised lesson content -->
				<div class="prose" aria-live="polite">{@html current.html}</div>
				<div class="nav">
					<Button variant="outline" disabled={step === 0} onclick={() => go(-1)}>← Step</Button>
					{#if step < steps.length - 1}
						<Button variant="dark" onclick={() => go(1)}>Step →</Button>
					{:else}
						<Button href="/drills/{lesson.drill}">Try the drill →</Button>
					{/if}
					<button
						type="button"
						class="toggle"
						aria-pressed={false}
						onclick={() => (showAll = true)}
					>
						Show all steps
					</button>
				</div>
				<p class="hint">Tip: use ← and → on your keyboard.</p>
			</Tile>
		{/if}

		<Tile area="drill" tone="lilac" label="Practice drill">
			<span class="pp-eyebrow lilac">Practice drill</span>
			<h2 class="drill-name">{lesson.drill.replace(/-/g, ' ')}</h2>
			<p>Lock it in with a short, graded exercise.</p>
			<div class="push">
				<Button href="/drills/{lesson.drill}" variant="dark">Start drill</Button>
			</div>
		</Tile>

		{#if data.next}
			<Tile area="next" href="/lessons/{data.next.id}">
				<span class="pp-eyebrow">Up next</span>
				<h2 class="next-title">{data.next.title}</h2>
				<p class="next-sum">{data.next.summary}</p>
				<span class="meta">{data.next.minutes} min · {skillLabel(data.next.skill)} →</span>
			</Tile>
		{/if}
	</div>
</div>

<style>
	.board {
		display: grid;
		gap: var(--pp-gap);
		grid-template-columns: repeat(4, minmax(0, 1fr));
		grid-template-areas:
			'title   title   title   learn'
			'diagram diagram step    step'
			'diagram diagram drill   next';
	}
	.board.has-why {
		grid-template-areas:
			'title   title   why     learn'
			'diagram diagram step    step'
			'diagram diagram drill   next';
	}
	/* "Show all steps": the step tile takes the diagram's place. */
	.board.all {
		grid-template-areas:
			'title title title learn'
			'step  step  step  step'
			'drill drill next  next';
	}
	.board.all.has-why {
		grid-template-areas:
			'title title why   learn'
			'step  step  step  step'
			'drill drill next  next';
	}
	/* Phones and small tablets: tiles stack in reading order. */
	@media (max-width: 900px) {
		.board.board {
			display: flex;
			flex-direction: column;
		}
	}

	h1 {
		font-size: clamp(36px, 5vw, 60px);
		line-height: 0.98;
	}
	h2 {
		font-size: 28px;
		letter-spacing: -0.6px;
	}
	.pp-eyebrow a {
		text-decoration: none;
	}
	.pp-eyebrow.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.pp-eyebrow.ink {
		color: var(--pp-ink);
	}
	.pp-eyebrow.lilac {
		color: var(--pp-lilac-ink);
	}
	p {
		margin: 0;
	}
	.lede {
		font-size: 18px;
		line-height: 1.5;
		color: var(--pp-ink-2);
	}
	.counter {
		margin-top: auto;
		display: flex;
		align-items: center;
		gap: 14px;
		font-weight: 600;
		font-size: 14px;
		white-space: nowrap;
	}
	.bar {
		flex: 1;
		height: 10px;
		border-radius: 999px;
		background: var(--pp-track);
		overflow: hidden;
	}
	.bar span {
		display: block;
		height: 100%;
		border-radius: inherit;
		background: var(--pp-accent);
		transition: width 0.35s var(--pp-spring);
	}

	.why {
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 24px;
		line-height: 1.15;
		letter-spacing: -0.4px;
	}
	.why-count {
		color: var(--pp-on-ink-muted);
		font-size: 15px;
		line-height: 1.45;
	}

	.learn {
		margin: 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-direction: column;
		gap: 10px;
		font-weight: 500;
		line-height: 1.35;
	}
	.learn li {
		display: flex;
		gap: 10px;
	}
	.learn li::before {
		content: '✓';
		flex: none;
		display: grid;
		place-items: center;
		width: 22px;
		height: 22px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: var(--pp-yellow);
		font-size: 12px;
		font-weight: 800;
	}

	.diagram {
		display: grid;
		place-items: center;
		flex: 1;
		color: var(--pp-ink);
	}
	.diagram :global(svg) {
		width: 100%;
		max-width: 520px;
		height: auto;
		overflow: visible;
	}
	.diagram.inline {
		margin: 8px 0 4px;
		padding: 20px;
		border-radius: var(--pp-radius-card);
		background: var(--pp-ground);
	}
	.diagram.inline :global(svg) {
		max-width: 420px;
	}
	.diagram :global(.accent) {
		stroke: var(--pp-accent);
	}
	.diagram :global(.faint) {
		stroke: var(--pp-muted);
		opacity: 0.7;
	}
	.diagram :global(.fill-yellow) {
		fill: var(--pp-yellow);
	}
	.diagram :global(.fill-lilac) {
		fill: var(--pp-lilac);
	}
	.diagram :global(.fill-paper) {
		fill: var(--pp-surface);
	}
	.diagram :global(.label) {
		font-family: var(--pp-font-body);
		font-weight: 600;
		stroke: none;
	}

	.prose {
		font-size: 17px;
		line-height: 1.6;
		color: var(--pp-ink-2);
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.prose :global(p) {
		margin: 0;
	}
	.prose :global(strong) {
		color: var(--pp-ink);
	}
	.nav {
		margin-top: auto;
		padding-top: 8px;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
	}
	.toggle,
	.step-head :global(.btn) {
		margin-left: auto;
	}
	.toggle {
		border: 0;
		background: none;
		padding: 8px 4px;
		font: 600 15px var(--pp-font-body);
		color: var(--pp-accent-ink);
		text-decoration: underline;
		text-underline-offset: 3px;
		cursor: pointer;
	}
	.hint {
		font-size: 13px;
		color: var(--pp-muted);
	}
	@media (hover: none) {
		.hint {
			display: none;
		}
	}
	.step-head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.visually-small {
		font-size: 20px;
	}
	.all-step {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding-top: 20px;
		border-top: 2px solid var(--pp-track);
	}

	.drill-name {
		font-size: 26px;
		text-transform: capitalize;
	}
	.push {
		margin-top: auto;
	}
	.next-title {
		font-size: 24px;
	}
	.next-sum {
		color: var(--pp-ink-2);
		font-size: 15px;
		line-height: 1.45;
	}
	.meta {
		margin-top: auto;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
</style>
