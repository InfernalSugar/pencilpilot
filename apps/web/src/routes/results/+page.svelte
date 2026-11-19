<script lang="ts">
	import { onMount } from 'svelte';
	import { dev } from '$app/env';
	import { page } from '$app/state';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import OverlayViewer, { type OverlayMode } from '#lib/results/OverlayViewer.svelte';
	import ScoreReveal from '#lib/results/ScoreReveal.svelte';
	import { loadSubmission, type Submission } from '#lib/submission.js';
	import { byId, imageUrl, tierName, tierOf, titleOf } from '#lib/references.js';
	import { attempts, recommendedTier, recurringMistakes, type Pattern } from '#lib/progress.js';
	import {
		humanize,
		measurement,
		type Feedback,
		type FeedbackItem,
		type Grade
	} from '#lib/grader.js';
	import {
		gradable,
		gradeSubmission,
		recordGraded,
		SKETCH_SKILLS,
		SKILLS,
		tipsFor,
		type Graded
	} from '#lib/grade-submission.js';
	import ReplayPlayer from '#lib/results/ReplayPlayer.svelte';
	import { percentile } from '#lib/daily.js';
	import { categories } from '#lib/categories.js';
	import { categoryAverage, personalStreak } from '#lib/personal-daily.js';
	import { sessionDrawing, timeLabel } from '#lib/timed.js';

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
	/** traced drawings are compared with the picture exactly where it was placed */
	let compareSrc = $state<string | null>(null);
	const traced = $derived(!!sub?.traced);
	let graded = $state<Graded | null>(null);
	let showReplay = $state(false);
	/** daily challenge: how this try compares with everyone's today */
	let dailyResult = $state<{
		counted: boolean;
		mine: number | null;
		beat: number | null;
		players: number;
		streak: number;
	} | null>(null);

	/** personal challenge: how today went against their own average */
	let personalResult = $state<{ avg: number | null; streak: number; category: string } | null>(
		null
	);

	async function countDaily(s: Submission, score: number) {
		if (!s.daily || s.traced) return;
		let counted = false;
		let mine: number | null = null;
		let streak = 0;
		if (page.data.user) {
			const res = await fetch('/api/daily', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({
					day: s.daily,
					refId: s.referenceId,
					score,
					attemptAt: s.at ?? null
				})
			}).catch(() => null);
			if (res?.ok) ({ counted, mine, streak } = await res.json());
		}
		const stats = await fetch(`/api/daily?day=${s.daily}`)
			.then((r) => (r.ok ? r.json() : null))
			.catch(() => null);
		const scores: number[] = stats?.scores ?? [];
		const mineOrThis = mine ?? score;
		dailyResult = {
			counted,
			mine,
			beat: Math.round(percentile(mineOrThis, mine === null ? [...scores, score] : scores) * 100),
			players: scores.length,
			streak: streak || stats?.streak || 0
		};
	}
	const canReplay = $derived(!!sub?.strokeData?.some((s) => s.points.length));
	let share = $state<{ state: 'idle' | 'busy' | 'done' | 'error'; message?: string }>({
		state: 'idle'
	});

	/** Community (flagged): share this drawing for human comparison + ratings. */
	async function shareDrawing() {
		if (!sub || !reference || !grade) return;
		share = { state: 'busy' };
		try {
			const res = await fetch('/api/community/share', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({
					png: sub.png,
					reference,
					score: grade.score,
					attemptAt: sub.at ?? null
				})
			});
			const body = await res.json().catch(() => ({}));
			share = res.ok
				? { state: 'done' }
				: { state: 'error', message: body.message ?? 'Couldn’t share that. Try again later.' };
		} catch {
			share = { state: 'error', message: 'Couldn’t reach the server. Try again later.' };
		}
	}

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

	const skills = $derived(
		grade
			? (sub?.mode === 'sketch' ? SKETCH_SKILLS : SKILLS)
					.map((s) => ({ ...s, value: measurement(grade!, s.name) ?? -1 }))
					.filter((s) => s.value >= 0)
			: []
	);
	const fixSkills = $derived(new Set(fb?.fixes.map((f) => f.skill) ?? []));

	onMount(async () => {
		const at = Number(page.url.searchParams.get('at'));
		sub = (at && sessionDrawing(at)) || loadSubmission();
		loaded = true;
		if (!sub || !gradable(sub)) return;
		try {
			const g = await gradeSubmission(sub);
			graded = g;
			grade = g.grade;
			void countDaily(sub, g.grade.score);
			fb = g.fb;
			compareSrc = g.compareSrc;
			// Tracing is practice: tips, but no score, progress, streak or level-up.
			// own pictures too: scored, but not part of category progress
			if (sub.traced || !g.ref || g.ref.own) return;
			const before = recommendedTier(g.ref.category);
			recordGraded(sub, g);
			if (sub.personal) {
				const all = attempts();
				personalResult = {
					avg: categoryAverage(all, g.ref.category, sub.at ?? Date.now()),
					streak: personalStreak(all),
					category: categories.find((c) => c.id === g.ref!.category)?.name ?? g.ref.category
				};
			}
			// a mistake that keeps coming back (and is in this drawing too)
			patterns = recurringMistakes(attempts()).filter((p) =>
				fb!.fixes.some((f) => f.id === p.ruleId)
			);
			const after = recommendedTier(g.ref.category);
			if (after > before) levelUp = after;
		} catch (e) {
			gradeError = `Could not grade this drawing: ${e}`;
		}
	});

	/** "Show me": switch the viewer to the view that explains a fix. */
	function showMe(f: FeedbackItem) {
		focused = f.id;
		mode = f.overlay === 'bbox' || f.overlay === 'offset' ? 'size' : f.overlay ? 'heatmap' : 'fade';
		document.getElementById('compare')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
	}

	const nextUrl = $derived(
		reference?.own
			? '/practice/own'
			: reference
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
					Results{reference?.own
						? ' · your own picture'
						: reference
							? ` · ${reference.category} · ${tierName(tierOf(reference))}`
							: ''}{sub.mode === 'sketch' ? ' · sketch' : ''}{sub.timeLimit
						? ` · ${timeLabel(sub.timeLimit)} ${sub.mode === 'sketch' ? 'sketch' : 'drawing'}`
						: ''}
				</span>
				<h1>{reference ? titleOf(reference) : 'Your drawing'}</h1>
			</div>
			<div class="actions">
				{#if reference}
					<Button
						href={reference.own
							? '/draw?own=1'
							: `/draw?category=${reference.category}&ref=${reference.id}`}
						variant="outline"
					>
						Try again
					</Button>
					<Button href={nextUrl} variant="dark">
						{reference.own ? 'Another picture →' : 'Next reference →'}
					</Button>
				{:else}
					<Button href="/draw" variant="outline">Draw another</Button>
				{/if}
			</div>
		</div>

		<div class="pp-bento grid">
			<Tile tone="ink">
				<span class="pp-eyebrow on-ink">Your score</span>
				{#if traced}
					{#if sub?.imported}
						<span class="big word">Not scored</span>
						<span class="note">
							It includes an imported picture, so it isn’t scored and doesn’t count toward your
							progress. Hide the picture layer before handing in to get a score.
						</span>
					{:else}
						<span class="big word">Traced</span>
						<span class="note">
							Tracing practice isn’t scored and doesn’t count toward your progress.
						</span>
					{/if}
				{:else if grade}
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
					{#if compareSrc || reference}
						<OverlayViewer
							referenceSrc={compareSrc ?? imageUrl(reference!)}
							drawingSrc={sub.png}
							{grade}
							bind:mode
							highlight={focused}
							aspect={sub.width / sub.height}
						/>
					{:else}
						<div
							class="frame"
							style="aspect-ratio: {sub.width} / {sub.height}; width: min(100%, calc(75vh * {sub.width /
								sub.height}))"
						>
							<img src={sub.png} alt="Your submitted drawing" />
						</div>
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
					{#if grade.flags.includes('unrelated')}
						<!-- coverage-style measures (big shapes, light & dark) reward any ink in the right area -->
						<p class="note">
							Your lines don’t follow the reference’s shapes yet, so there’s nothing to break down.
							Start with its main outline.
						</p>
					{:else}
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
					{/if}
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

		{#if dailyResult && sub}
			<section class="daily" aria-label="Today’s challenge">
				<span class="pp-eyebrow">Today’s challenge</span>
				<p class="daily-line">
					{#if dailyResult.counted}
						{#if dailyResult.players > 1}
							Counted! Better than <b>{dailyResult.beat}%</b> of today’s
							{dailyResult.players - 1} other players.
						{:else}
							Counted! You’re the first one in today.
						{/if}
					{:else if dailyResult.mine !== null}
						Your first try today ({dailyResult.mine.toFixed(1)}) is the one that counts. This one
						would beat {dailyResult.beat}% of today’s players.
					{:else if dailyResult.players}
						This would beat {dailyResult.beat}% of today’s {dailyResult.players} players.
						<a href="/login?next=/daily">Sign in</a> to be counted.
					{:else}
						No one has been counted yet today. <a href="/login?next=/daily">Sign in</a> to be the first.
					{/if}
				</p>
				<p class="daily-meta">
					{#if dailyResult.streak}Daily streak: <b>{dailyResult.streak}</b> ·{/if}
					<a href="/daily">Today’s challenge page</a>
				</p>
			</section>
		{/if}

		{#if personalResult && grade}
			{@const diff = personalResult.avg === null ? null : grade.score - personalResult.avg}
			<section class="daily personal" aria-label="Your challenge">
				<span class="pp-eyebrow">Your challenge · done</span>
				<p class="daily-line">
					{#if diff === null}
						Your first {personalResult.category} drawing: this sets the bar.
					{:else if diff >= 0.05}
						<b>+{diff.toFixed(1)}</b> above your {personalResult.category} average ({personalResult.avg!.toFixed(
							1
						)}). Nice!
					{:else if diff > -0.05}
						Right on your {personalResult.category} average ({personalResult.avg!.toFixed(1)}).
					{:else}
						{Math.abs(diff).toFixed(1)} below your {personalResult.category} average ({personalResult.avg!.toFixed(
							1
						)}). Tricky one: the tips below show why.
					{/if}
				</p>
				<p class="daily-meta">
					Personal streak: <b>{personalResult.streak}</b> day{personalResult.streak === 1
						? ''
						: 's'} · <a href="/daily">Tomorrow’s is waiting there</a>
				</p>
			</section>
		{/if}

		{#if canReplay}
			<section class="replay-section" aria-label="Replay">
				<div class="replay-head">
					<h2>Replay</h2>
					{#if !showReplay}
						<Button variant="dark" onclick={() => (showReplay = true)}>Watch the replay ▶</Button>
					{/if}
				</div>
				{#if showReplay && sub}
					<ReplayPlayer
						{sub}
						tips={fb?.fixes.map((f) => ({ id: f.id, title: f.title })) ?? []}
						firedAt={graded && sub
							? (gray, strokes) => tipsFor(sub!, graded!, gray, strokes)
							: undefined}
					/>
				{:else}
					<p class="muted-line">
						See your drawing come together stroke by stroke{fb?.fixes.length
							? ', with a mark where each tip’s mistake started'
							: ''}.
					</p>
				{/if}
			</section>
		{/if}

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
		{#if page.data.community && grade && reference && !reference.own && !traced && !grade.flags.includes('blank')}
			<div class="share">
				<Tile tone="lilac" as="section" label="Get a human opinion">
					<span class="pp-eyebrow">Community · beta</span>
					<h2>Get a human opinion</h2>
					{#if !page.data.user}
						<p>Sign in to share this drawing. Other students compare and rate it.</p>
						<div><Button href="/login?next=/results" variant="dark">Sign in</Button></div>
					{:else if share.state === 'done'}
						<p role="status">
							Shared! See how it does in <a href="/community/mine">My shared drawings</a>.
						</p>
					{:else}
						<p>
							Other signed-in students will compare it with drawings of the same picture and rate
							it. They won’t see your name. You can delete it any time.
						</p>
						{#if share.state === 'error'}<p class="share-err" role="alert">{share.message}</p>{/if}
						<div>
							<Button variant="dark" onclick={shareDrawing} disabled={share.state === 'busy'}>
								{share.state === 'busy' ? 'Checking…' : 'Share with the community'}
							</Button>
						</div>
					{/if}
				</Tile>
			</div>
		{/if}
		{#if reference?.own}
			<p class="credit">
				Your own picture (kept on this device only) · {time}, {sub.strokes} strokes
			</p>
		{:else if reference}
			<p class="credit">
				Reference by <a href={reference.authorUrl} target="_blank" rel="noreferrer">
					{reference.author}
				</a>
				via <a href={reference.pageUrl} target="_blank" rel="noreferrer">Pixabay</a> ·
				<a href={reference.licenseUrl} target="_blank" rel="noreferrer">{reference.license}</a> ·
				{time}, {sub.strokes} strokes
			</p>
		{/if}
		{#if dev && grade && reference && !traced}
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
	.daily {
		margin-top: var(--pp-gap);
		padding: 24px 28px;
		border-radius: var(--pp-radius-tile);
		background: var(--pp-yellow);
		color: var(--pp-ink);
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.daily.personal {
		background: var(--pp-lilac);
	}
	.daily-line {
		margin: 0;
		font: 600 20px var(--pp-font-body);
	}
	.daily-meta {
		margin: 0;
		font-size: 14px;
	}
	.replay-section {
		margin-top: var(--pp-gap);
		padding: 28px;
		border-radius: var(--pp-radius-tile);
		background: var(--pp-surface);
		display: flex;
		flex-direction: column;
		gap: 14px;
	}
	.replay-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}
	.replay-head h2 {
		font-size: 22px;
	}
	.muted-line {
		margin: 0;
		color: var(--pp-muted);
	}
	.share {
		margin-top: var(--pp-gap);
	}
	.share h2 {
		font-size: 22px;
	}
	.share p {
		margin: 0;
		max-width: 60ch;
	}
	.share-err {
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
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
		margin-inline: auto;
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
	.big.word {
		font-size: 56px;
		letter-spacing: -2px;
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
