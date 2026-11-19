<script lang="ts">
	import { onMount } from 'svelte';
	import { dev } from '$app/env';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import OverlayViewer, { type OverlayMode } from '#lib/results/OverlayViewer.svelte';
	import ScoreReveal from '#lib/results/ScoreReveal.svelte';
	import { loadSubmission, type Submission } from '#lib/submission.js';
	import { byId, imageUrl, referencePixels, tierName, tierOf, titleOf } from '#lib/references.js';
	import {
		attempts,
		recommendedTier,
		recordAttempt,
		recurringMistakes,
		saveThumb,
		type Pattern
	} from '#lib/progress.js';
	import {
		getFeedback,
		gradeImages,
		humanize,
		measurement,
		toGray,
		type Feedback,
		type FeedbackItem,
		type Grade,
		type GradeCategory,
		type MeasurementName
	} from '#lib/grader.js';

	let sub = $state<Submission | null>(null);
	let loaded = $state(false);
	let grade = $state<Grade | null>(null);
	let fb = $state<Feedback | null>(null);
	let gradeError = $state('');
	let levelUp = $state(0);
	let mode = $state<OverlayMode>('fade');
	let focused = $state<string | null>(null);
	let patterns = $state<Pattern[]>([]);
	let votes = $state<Record<string, 1 | -1>>({});
	let calSaved = $state(false);

	/** Dev only: keep this drawing + grade as a calibration sample for /dev/rate. */
	async function saveForCalibration() {
		if (!sub || !reference || !grade) return;
		const res = await fetch('/dev/calibration', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				category: reference.category,
				refId: reference.id,
				refUrl: imageUrl(reference),
				similarity: grade.similarity,
				score: grade.score,
				png: sub.png
			})
		});
		calSaved = res.ok;
	}

	async function vote(f: FeedbackItem, v: 1 | -1) {
		votes[f.id] = v;
		await fetch('/api/tip-vote', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				ruleId: f.id,
				vote: v,
				category: reference?.category,
				score: grade?.score,
				attemptAt: sub?.at
			})
		}).catch(() => {});
	}
	const reference = $derived(sub?.reference ?? byId(sub?.referenceId));

	const time = $derived(
		sub ? `${Math.floor(sub.seconds / 60)}m ${String(sub.seconds % 60).padStart(2, '0')}s` : ''
	);

	/** The skills shown in the breakdown, in plain words. */
	const SKILLS: { name: MeasurementName; label: string; skill: string }[] = [
		{ name: 'placement', label: 'Size & placement', skill: 'placement' },
		{ name: 'outline', label: 'Outlines', skill: 'outline' },
		{ name: 'silhouette', label: 'Big shapes', skill: 'silhouette' },
		{ name: 'tone', label: 'Light & dark', skill: 'tone' },
		{ name: 'line_confidence', label: 'Line confidence', skill: 'line_confidence' }
	];
	const skills = $derived(
		grade
			? SKILLS.map((s) => ({ ...s, value: measurement(grade!, s.name) ?? -1 })).filter(
					(s) => s.value >= 0
				)
			: []
	);
	const fixSkills = $derived(new Set(fb?.fixes.map((f) => f.skill) ?? []));

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
			fb = await getFeedback(grade, sub.at ?? 0, ref.category);
			const tier = sub.tier ?? tierOf(ref);
			const before = recommendedTier(ref.category);
			if (!alreadyRecorded(sub.at)) {
				recordAttempt({
					category: ref.category,
					topic: sub.topic ?? ref.topic ?? null,
					tier,
					score: grade.score,
					at: sub.at ?? Date.now(),
					refId: ref.id,
					title: titleOf(ref),
					fixes: fb.fixes.map((f) => f.id),
					skills: Object.fromEntries(
						SKILLS.map((s) => [s.skill, measurement(grade!, s.name)]).filter(
							([, v]) => v !== undefined
						)
					)
				});
				if (sub.at) saveThumb(sub.at, sub.png);
			}
			// a mistake that keeps coming back (and is in this drawing too)
			patterns = recurringMistakes(attempts()).filter((p) =>
				fb!.fixes.some((f) => f.id === p.ruleId)
			);
			const after = recommendedTier(ref.category);
			if (after > before) levelUp = after;
		} catch (e) {
			gradeError = `Could not grade this drawing: ${e}`;
		}
	});

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

	/** "Show me": switch the viewer to the view that explains a fix. */
	function showMe(f: FeedbackItem) {
		focused = f.id;
		mode = f.overlay === 'bbox' || f.overlay === 'offset' ? 'size' : f.overlay ? 'heatmap' : 'fade';
		document.getElementById('compare')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
	}

	const nextUrl = $derived(
		reference
			? `/draw?category=${reference.category}${sub?.topic ? `&topic=${sub.topic}` : ''}&tier=${sub?.tier ?? tierOf(reference)}`
			: '/draw'
	);
	const pct = (v: number) => `${Math.round(v * 100)}%`;
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
				<span class="pp-eyebrow">
					Results{reference ? ` · ${reference.category} · ${tierName(tierOf(reference))}` : ''}
				</span>
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
					<ScoreReveal score={grade.score} />
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

			<Tile span={2} rows={2} as="section" label="Compare">
				<div id="compare">
					{#if reference}
						<OverlayViewer
							referenceSrc={imageUrl(reference)}
							drawingSrc={sub.png}
							{grade}
							bind:mode
							highlight={focused}
						/>
					{:else}
						<div class="frame"><img src={sub.png} alt="Your submitted drawing" /></div>
					{/if}
				</div>
			</Tile>

			{#if fb?.praise}
				<Tile tone="yellow">
					<span class="pp-eyebrow ink">You nailed it</span>
					<span class="mid">{fb.praise.title}</span>
					<span class="note">{fb.praise.text}</span>
				</Tile>
			{:else}
				<Tile>
					<span class="pp-eyebrow">Time · strokes</span>
					<span class="mid">{time}</span>
					<span class="note">{sub.strokes} strokes, {sub.brushes.length} brush(es)</span>
				</Tile>
			{/if}

			{#if grade}
				<Tile>
					<h2>Breakdown</h2>
					<ul class="bars">
						{#each skills as s (s.name)}
							<li>
								<span class="row"><span>{s.label}</span><b>{pct(s.value)}</b></span>
								<span class="track">
									<span
										class="fill"
										class:low={fixSkills.has(s.skill)}
										style="width: {pct(s.value)}"
									></span>
								</span>
							</li>
						{/each}
					</ul>
				</Tile>
			{/if}
			{#if fb?.praise}
				<Tile>
					<span class="pp-eyebrow">Time · strokes</span>
					<span class="mid">{time}</span>
					<span class="note">{sub.strokes} strokes, {sub.brushes.length} brush(es)</span>
				</Tile>
			{/if}
		</div>

		{#if levelUp && reference}
			<div class="pp-bento levelup">
				<Tile tone="accent" span={4}>
					<span class="pp-eyebrow on-accent">Level up!</span>
					<p class="big-line light">
						Three strong drawings in a row. You’re ready for {tierName(levelUp)}.
					</p>
					<div>
						<Button href="/practice/{reference.category}?tier={levelUp}" variant="dark">
							Try {tierName(levelUp)} →
						</Button>
					</div>
				</Tile>
			</div>
		{/if}

		{#if patterns.length && fb}
			{@const p = patterns[0]}
			{@const fix = fb.fixes.find((f) => f.id === p.ruleId)}
			{#if fix}
				<div class="pp-bento levelup">
					<Tile tone="lilac" span={4}>
						<span class="pp-eyebrow lilac-ink">Pattern spotted</span>
						<p class="big-line">
							“{fix.title}” came up in {p.count} of your last {p.window} drawings.
						</p>
						<div class="fix-actions">
							{#if fix.lesson}
								<Button href="/lessons/{fix.lesson}?from={fix.id}" variant="dark">
									Fix it: {humanize(fix.lesson)} →
								</Button>
							{/if}
							{#if fix.drill}
								<Button href="/drills/{fix.drill}" variant="outline">Drill it</Button>
							{/if}
						</div>
					</Tile>
				</div>
			{/if}
		{/if}

		{#if fb}
			<h2 class="section">{fb.fixes.length ? 'What to work on next' : 'Nothing big to fix'}</h2>
			<div class="fixes">
				{#each fb.fixes as f, i (f.id)}
					<Tile as="article" label="Fix {i + 1}">
						<div class="fix-head">
							<span class="num" class:first={i === 0}>{i + 1}</span>
							<h3>{f.title}</h3>
						</div>
						<p class="fix-text">{f.text}</p>
						<div class="fix-actions">
							{#if f.overlay}
								<button type="button" class="chip dark" onclick={() => showMe(f)}>Show me</button>
							{/if}
							{#if f.lesson}
								<a class="chip" href="/lessons/{f.lesson}?from={f.id}">
									Lesson: {humanize(f.lesson)}
								</a>
							{/if}
							{#if f.drill}
								<a class="chip" href="/drills/{f.drill}">Drill: {humanize(f.drill)}</a>
							{/if}
						</div>
						<div class="vote" role="group" aria-label="Was this tip helpful?">
							{#if votes[f.id]}
								<span class="thanks" role="status">Thanks for the feedback!</span>
							{:else}
								<span>Helpful?</span>
								<button type="button" aria-label="Helpful" onclick={() => vote(f, 1)}>👍</button>
								<button type="button" aria-label="Not helpful" onclick={() => vote(f, -1)}>
									👎
								</button>
							{/if}
						</div>
					</Tile>
				{:else}
					{#if !grade?.flags.includes('blank')}
						<Tile tone="lilac">
							<p class="big-line">Great match! Try a harder level or a shorter time next.</p>
						</Tile>
					{/if}
				{/each}
			</div>
		{/if}
		{#if reference}
			<p class="credit">
				Reference by <a href={reference.authorUrl} target="_blank" rel="noreferrer">
					{reference.author}
				</a>
				via <a href={reference.pageUrl} target="_blank" rel="noreferrer">Pixabay</a> ·
				<a href={reference.licenseUrl} target="_blank" rel="noreferrer">{reference.license}</a> ·
				{time}, {sub.strokes} strokes
			</p>
		{/if}
		{#if dev && grade && reference}
			<p class="credit">
				<button type="button" class="chip" onclick={saveForCalibration} disabled={calSaved}>
					{calSaved ? 'Saved for calibration ✓' : 'Save for calibration (dev)'}
				</button>
				· <a href="/dev/rate">Rate drawings</a> · <a href="/dev/feedback">Tip ratings</a>
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
		font-size: 22px;
		font-weight: 700;
		letter-spacing: -0.4px;
		line-height: 1.15;
	}
	.grid {
		grid-auto-rows: minmax(200px, auto);
	}
	.frame {
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
		font-size: 26px;
		line-height: 1.1;
		margin-top: auto;
	}
	.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.on-accent {
		color: rgba(255, 255, 255, 0.85);
	}
	.ink {
		color: var(--pp-ink);
	}
	.note {
		font-size: 14px;
		line-height: 1.45;
		opacity: 0.9;
	}
	.big-line {
		margin: 0;
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 24px;
		line-height: 1.2;
		color: var(--pp-ink);
	}
	.big-line.light {
		color: #fff;
	}
	.section {
		margin: 32px 0 12px;
	}
	.levelup {
		margin-top: var(--pp-gap);
	}
	.fixes {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
		gap: var(--pp-gap);
	}
	h3 {
		font-size: 22px;
		font-weight: 700;
		letter-spacing: -0.4px;
		line-height: 1.15;
	}
	.fix-head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.num {
		flex: none;
		width: 36px;
		height: 36px;
		display: grid;
		place-items: center;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 700;
	}
	.num.first {
		background: var(--pp-accent-strong);
	}
	.fix-text {
		margin: 0;
		font-size: 16px;
		line-height: 1.55;
		color: var(--pp-ink-2);
	}
	.fix-actions {
		margin-top: auto;
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.chip {
		padding: 8px 14px;
		border-radius: 12px;
		border: 0;
		background: var(--pp-ground);
		font: 600 13px var(--pp-font-body);
		color: var(--pp-ink);
	}
	a.chip {
		text-decoration: none;
	}
	a.chip:hover {
		background: var(--pp-lilac);
		color: var(--pp-ink);
	}
	.lilac-ink {
		color: var(--pp-lilac-ink);
	}
	.vote {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.vote button {
		width: 36px;
		height: 36px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ground);
		cursor: pointer;
		font-size: 16px;
	}
	.vote button:hover {
		background: var(--pp-lilac);
	}
	.thanks {
		color: var(--pp-cat-env);
		font-weight: 600;
	}
	.chip.dark {
		background: var(--pp-ink);
		color: #fff;
		cursor: pointer;
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
	}
</style>
