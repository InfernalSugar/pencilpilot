<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { loadSubmission, type Submission } from '#lib/submission.js';
	import { byId, imageUrl, referencePixels, tierName, tierOf, titleOf } from '#lib/references.js';
	import { recommendedTier, recordAttempt } from '#lib/progress.js';
	import {
		gradeImages,
		measurement,
		toGray,
		type Grade,
		type GradeCategory,
		type MeasurementName
	} from '#lib/grader.js';

	let sub = $state<Submission | null>(null);
	let loaded = $state(false);
	let grade = $state<Grade | null>(null);
	let gradeError = $state('');
	const reference = $derived(sub?.reference ?? byId(sub?.referenceId));

	const time = $derived(
		sub ? `${Math.floor(sub.seconds / 60)}m ${String(sub.seconds % 60).padStart(2, '0')}s` : ''
	);

	/** The skills shown in the breakdown, in plain words. */
	const SKILLS: { name: MeasurementName; label: string }[] = [
		{ name: 'placement', label: 'Size & placement' },
		{ name: 'outline', label: 'Outlines' },
		{ name: 'silhouette', label: 'Big shapes' },
		{ name: 'tone', label: 'Light & dark' },
		{ name: 'line_confidence', label: 'Line confidence' }
	];
	const skills = $derived(
		grade
			? SKILLS.map((s) => ({ ...s, value: measurement(grade!, s.name) ?? -1 })).filter(
					(s) => s.value >= 0
				)
			: []
	);
	const best = $derived(
		skills.length ? skills.reduce((a, b) => (b.value > a.value ? b : a)) : null
	);
	const worst = $derived(
		skills.length ? skills.reduce((a, b) => (b.value < a.value ? b : a)) : null
	);

	/** A first, simple hint until the full feedback engine (M4) lands. */
	const hint = $derived.by(() => {
		if (!grade) return '';
		if (grade.flags.includes('blank')) return 'The canvas looks empty. Give it another go!';
		const scale = measurement(grade, 'scale_ratio');
		if (scale !== undefined && scale > 1.15)
			return `Your drawing is about ${Math.round((scale - 1) * 100)}% bigger than the reference. Block in the outer edges first, then fill in.`;
		if (scale !== undefined && scale < 0.85)
			return `Your drawing is about ${Math.round((1 - scale) * 100)}% smaller than the reference. Try using more of the canvas.`;
		const aspect = measurement(grade, 'aspect_error');
		if (aspect !== undefined && Math.abs(aspect) > 0.12)
			return `Your drawing is ${aspect > 0 ? 'wider' : 'narrower'} than the reference. Compare its width to its height before adding detail.`;
		if (worst && worst.value < 0.6)
			return `${worst.label} is your weakest area this time. Slow down there and compare with the reference often.`;
		return 'Nice work! Try a harder reference or a shorter time next.';
	});

	async function decodeGray(png: string, w: number, h: number) {
		const img = new Image();
		img.src = png;
		await img.decode();
		const c = new OffscreenCanvas(w, h);
		const ctx = c.getContext('2d')!;
		ctx.drawImage(img, 0, 0, w, h);
		return toGray(ctx.getImageData(0, 0, w, h).data);
	}

	onMount(async () => {
		sub = loadSubmission();
		loaded = true;
		const ref = sub?.reference ?? byId(sub?.referenceId);
		if (!sub || !ref) return;
		try {
			const [drawing, refData] = await Promise.all([
				decodeGray(sub.png, sub.width, sub.height),
				referencePixels(ref, sub.width, sub.height)
			]);
			grade = await gradeImages(sub.width, sub.height, drawing, toGray(refData.data), {
				// Cartoons are graded like anime: clean outlines matter most.
				category: (ref.category === 'cartoon' ? 'anime' : ref.category) as GradeCategory,
				strokes: sub.strokeData ?? []
			});
			const tier = sub.tier ?? tierOf(ref);
			const before = recommendedTier(ref.category);
			if (!alreadyRecorded(sub.at)) {
				recordAttempt({
					category: ref.category,
					topic: sub.topic ?? null,
					tier,
					score: grade.score,
					at: sub.at ?? Date.now()
				});
			}
			const after = recommendedTier(ref.category);
			if (after > before) levelUp = after;
		} catch (e) {
			gradeError = `Could not grade this drawing: ${e}`;
		}
	});

	let levelUp = $state(0);

	/** Reloading the results page must not count the same drawing twice. */
	function alreadyRecorded(at: number | undefined) {
		if (!at) return false;
		try {
			const seen: number[] = JSON.parse(sessionStorage.getItem('pp.recorded') ?? '[]');
			if (seen.includes(at)) return true;
			sessionStorage.setItem('pp.recorded', JSON.stringify([...seen, at].slice(-50)));
		} catch {
			/* storage blocked: may double count on reload, harmless */
		}
		return false;
	}

	const nextUrl = $derived(
		reference
			? `/draw?category=${reference.category}${sub?.topic ? `&topic=${sub.topic}` : ''}&tier=${sub?.tier ?? tierOf(reference)}`
			: '/draw'
	);

	const pct = (v: number) => `${Math.round(v * 100)}%`;
	const worstCell = $derived(
		grade?.grid.worst && grade.grid.cells[grade.grid.worst.index] < 0.75 ? grade.grid.worst : null
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
				<span class="pp-eyebrow">Results{reference ? ` · ${reference.category}` : ''}</span>
				<h1>{reference ? titleOf(reference) : 'Your drawing'}</h1>
			</div>
			<div class="actions">
				{#if reference}
					<Button href="/draw?category={reference.category}&ref={reference.id}" variant="outline">
						Try again
					</Button>
					<Button href={nextUrl} variant="dark">Next reference →</Button>
				{:else}
					<Button href="/draw" variant="outline">Draw another</Button>
				{/if}
			</div>
		</div>

		<div class="pp-bento grid">
			<Tile tone="ink">
				<span class="pp-eyebrow on-ink">Your score</span>
				{#if grade}
					<span class="big" data-testid="score">{grade.score.toFixed(1)}</span>
					<span class="note">{grade.passed ? 'Passed! Great match.' : 'out of 10'}</span>
				{:else if reference && !gradeError}
					<span class="big">…</span>
					<span class="note">Measuring your drawing…</span>
				{:else}
					<span class="big">–</span>
					<span class="note">
						{gradeError || 'Free drawings aren’t scored. Pick a category to copy a reference.'}
					</span>
				{/if}
			</Tile>

			<Tile span={2} rows={2}>
				<div class="pair" class:single={!reference}>
					{#if reference}
						<figure>
							<div class="frame"><img src={imageUrl(reference)} alt="The reference" /></div>
							<figcaption>Reference</figcaption>
						</figure>
					{/if}
					<figure>
						<div class="frame">
							<img src={sub.png} alt="Your submitted drawing" />
							{#if grade && worstCell}
								<span
									class="cell"
									title="The area that differs most from the reference"
									style="left: {(worstCell.col / grade.grid.cols) * 100}%; top: {(worstCell.row /
										grade.grid.rows) *
										100}%; width: {100 / grade.grid.cols}%; height: {100 / grade.grid.rows}%"
								></span>
							{/if}
						</div>
						<figcaption>
							Your drawing{worstCell ? ' · red box = biggest difference' : ''}
						</figcaption>
					</figure>
				</div>
			</Tile>

			{#if grade}
				<Tile>
					<h2>Breakdown</h2>
					<ul class="bars">
						{#each skills as s (s.name)}
							<li>
								<span class="row"><span>{s.label}</span><b>{pct(s.value)}</b></span>
								<span class="track">
									<span class="fill" class:low={s === worst} style="width: {pct(s.value)}"></span>
								</span>
							</li>
						{/each}
					</ul>
				</Tile>
				<Tile tone="yellow">
					<span class="pp-eyebrow ink">You nailed</span>
					<span class="mid">{best?.label}</span>
					<span class="note">{best ? pct(best.value) : ''} match</span>
				</Tile>
				{#if levelUp && reference}
					<Tile tone="accent" span={2}>
						<span class="pp-eyebrow on-accent">Level up!</span>
						<p class="tip light">
							Three strong drawings in a row. You’re ready for {tierName(levelUp)}.
						</p>
						<div>
							<Button href="/practice/{reference.category}?tier={levelUp}" variant="dark">
								Try {tierName(levelUp)} →
							</Button>
						</div>
					</Tile>
				{/if}
				<Tile tone="lilac" span={2}>
					<span class="pp-eyebrow lilac">Tip</span>
					<p class="tip">{hint}</p>
					<span class="note">
						Detailed tips, lessons and drills arrive in the next update (M4).
					</span>
				</Tile>
			{/if}
			<Tile>
				<span class="pp-eyebrow">Time · strokes</span>
				<span class="mid">{time}</span>
				<span class="note">
					{sub.strokes} strokes, {sub.points.toLocaleString()} pen samples, {sub.brushes.length}
					brush(es)
				</span>
			</Tile>
		</div>
		{#if reference}
			<p class="credit">
				Reference by <a href={reference.authorUrl} target="_blank" rel="noreferrer">
					{reference.author}
				</a>
				via <a href={reference.pageUrl} target="_blank" rel="noreferrer">Pixabay</a> ·
				<a href={reference.licenseUrl} target="_blank" rel="noreferrer">{reference.license}</a>
			</p>
		{/if}
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
	.actions {
		display: flex;
		gap: 12px;
		flex-wrap: wrap;
	}
	h1 {
		font-size: 30px;
	}
	h2 {
		font-size: 20px;
		font-weight: 700;
	}
	.grid {
		grid-auto-rows: minmax(220px, auto);
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}
	.pair.single {
		grid-template-columns: 1fr;
	}
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	figcaption {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.frame {
		position: relative;
		aspect-ratio: 4 / 3;
		background: #fff;
		border-radius: 20px;
		overflow: hidden;
		border: 1px solid var(--pp-track);
	}
	.frame img {
		width: 100%;
		height: 100%;
		object-fit: contain;
		display: block;
	}
	.cell {
		position: absolute;
		border: 3px solid var(--pp-accent);
		border-radius: 10px;
		pointer-events: none;
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 96px;
		line-height: 0.85;
		letter-spacing: -4px;
		margin-top: auto;
	}
	.mid {
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 30px;
		line-height: 1.05;
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
	.on-accent {
		color: rgba(255, 255, 255, 0.85);
	}
	.tip.light {
		color: #fff;
	}
	.tip {
		margin: 0;
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 24px;
		line-height: 1.2;
		color: var(--pp-ink);
	}
	.bars {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 10px;
		font-size: 14px;
	}
	.row {
		display: flex;
		justify-content: space-between;
	}
	.track {
		display: block;
		height: 8px;
		margin-top: 4px;
		background: var(--pp-track);
		border-radius: 999px;
	}
	.fill {
		display: block;
		height: 100%;
		background: var(--pp-ink);
		border-radius: 999px;
	}
	.fill.low {
		background: var(--pp-accent);
	}
	.credit {
		margin: 16px 0 0;
		font-size: 12px;
		color: var(--pp-muted);
	}
	p {
		margin: 0;
		color: var(--pp-ink-2);
	}
</style>
