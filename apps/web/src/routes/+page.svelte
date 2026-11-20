<script lang="ts">
	import { dailyReference, dayOf } from '#lib/daily.js';
	import { imageUrl, titleOf } from '#lib/references.js';
	const todayRef = dailyReference(dayOf());
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import { onMount } from 'svelte';
	import { categories } from '#lib/categories.js';
	import { liveAvailable } from '#lib/live.js';
	import { attempts, streakDays, type Attempt } from '#lib/progress.js';
	import { SYNCED } from '#lib/sync.js';

	/** With Endless references every category is drawable. */
	let live = $state(false);
	onMount(() => {
		history = attempts();
		const reload = () => (history = attempts());
		window.addEventListener(SYNCED, reload);
		liveAvailable().then((ok) => (live = ok));
		return () => window.removeEventListener(SYNCED, reload);
	});

	const days = [0, 1, 2, 3, 4, 5, 6];
	let history = $state<Attempt[]>([]);
	const last = $derived(history.at(-1));
	const streak = $derived(streakDays(history));
</script>

<svelte:head>
	<title>PencilPilot · Draw it. Score it. Get better.</title>
	<meta
		name="description"
		content="Practise drawing from references and get a score out of 10 with tips on what to fix."
	/>
</svelte:head>

<div class="pp-page">
	<div class="pp-bento hero-grid">
		<Tile span={2} rows={2} as="section">
			<svg class="blob" width="220" height="220" viewBox="0 0 220 220" aria-hidden="true">
				<circle cx="110" cy="110" r="100" fill="var(--pp-accent)" />
				<path
					d="M60 120 C 90 70, 140 70, 165 115"
					fill="none"
					stroke="#fff"
					stroke-width="7"
					stroke-linecap="round"
				/>
			</svg>
			<span class="pp-eyebrow">Daily drawing practice</span>
			<h1 class="hero-title">Copy it.<br />Score it.<br /><span>Get better.</span></h1>
			<p class="lede">
				Draw from a reference and get an honest score out of 10, plus the fixes that matter most.
			</p>
			<div class="actions">
				<Button href="/draw">Start drawing →</Button>
				<Button href="#how" variant="outline">How it works</Button>
			</div>
		</Tile>

		<Tile tone="ink">
			<span class="pp-eyebrow on-ink">Last score</span>
			{#if last}
				<span class="big">{last.score.toFixed(1)}</span>
				<span class="note">
					{last.title ?? last.category} · <a href="/progress">See progress →</a>
				</span>
			{:else}
				<span class="big">–</span>
				<span class="note">Your first score shows up here after you submit a drawing.</span>
			{/if}
		</Tile>

		<Tile tone="yellow">
			<span class="pp-eyebrow ink">Streak</span>
			<span class="big">{streak}<small> {streak === 1 ? 'day' : 'days'}</small></span>
			<div class="streak" aria-hidden="true">
				{#each days as d (d)}<span class:on={d < Math.min(streak, 7)}></span>{/each}
			</div>
		</Tile>

		<Tile tone="lilac" span={2}>
			<div class="challenge">
				{#if todayRef}
					<img class="ref-ph" src={imageUrl(todayRef)} alt="Today’s picture: {titleOf(todayRef)}" />
				{:else}
					<div class="ref-ph">Reference</div>
				{/if}
				<div class="challenge-body">
					<span class="pp-eyebrow lilac">Today’s challenge</span>
					<h2 class="challenge-title">
						{todayRef ? titleOf(todayRef) : 'A new picture every day'}
					</h2>
					<span class="note">The same picture for everyone. See how you compare.</span>
					<div class="row-btns">
						<Button href="/daily" variant="dark">Today’s challenge →</Button>
						<Button href="/daily#yours" variant="outline">Your own challenge</Button>
					</div>
				</div>
			</div>
		</Tile>
	</div>

	<div class="pp-bento together">
		<Tile tone="yellow" span={4}>
			<div class="together-row">
				<div>
					<span class="pp-eyebrow ink">New · Private lobbies</span>
					<h2 class="challenge-title">Draw with friends</h2>
					<span class="note">
						Race the clock on the same picture, or share one canvas and draw live.
					</span>
				</div>
				<Button href="/lobby" variant="dark">Draw together →</Button>
			</div>
		</Tile>
	</div>

	<h2 class="section-title">Choose what to draw</h2>
	<div class="cats">
		{#each categories as c (c.id)}
			<a
				class="cat"
				class:dark={c.dark}
				href={c.id === 'fundamentals'
					? '/drills'
					: c.available || live
						? `/practice/${c.id}`
						: undefined}
				aria-disabled={c.id !== 'fundamentals' && !c.available && !live}
			>
				<span class="dot" style="background: {c.color}"></span>
				<span class="cat-text">
					<span class="cat-name">{c.name}</span>
					<span class="cat-blurb">
						{c.id === 'fundamentals' || c.available || live ? c.blurb : 'Coming soon'}
					</span>
				</span>
			</a>
		{/each}
		<a class="cat own" href="/practice/own">
			<span class="dot own-dot" aria-hidden="true"></span>
			<span class="cat-text">
				<span class="cat-name">Your own picture</span>
				<span class="cat-blurb">Draw from any picture you have</span>
			</span>
		</a>
	</div>

	<h2 class="section-title" id="how">How it works</h2>
	<div class="pp-bento">
		<Tile>
			<span class="step">1</span>
			<h3>Pick a reference</h3>
			<p>Choose a category. Every picture is royalty-free and safe for all ages.</p>
		</Tile>
		<Tile>
			<span class="step">2</span>
			<h3>Draw it</h3>
			<p>Mouse, pen or tablet, with a full set of brushes and an optional timer.</p>
		</Tile>
		<Tile>
			<span class="step">3</span>
			<h3>Get your score</h3>
			<p>We measure proportions, angles, shapes, shading and line confidence. No AI involved.</p>
		</Tile>
		<Tile tone="ink">
			<span class="step light">4</span>
			<h3>Level up</h3>
			<p class="on-ink-p">Each mistake links to a short lesson and a drill to fix it.</p>
		</Tile>
	</div>
</div>

<style>
	.hero-grid {
		grid-template-rows: minmax(300px, auto) minmax(260px, auto);
	}
	.blob {
		position: absolute;
		right: -40px;
		top: -40px;
	}
	@media (max-width: 560px) {
		.blob {
			width: 120px;
			height: 120px;
			right: -36px;
			top: -36px;
		}
	}
	.hero-title {
		font-size: clamp(52px, 7vw, 88px);
		line-height: 0.92;
		letter-spacing: -3.5px;
		position: relative;
		margin: auto 0;
	}
	.hero-title span {
		color: var(--pp-accent);
	}
	.lede {
		margin: 0;
		max-width: 440px;
		font-size: 18px;
		line-height: 1.55;
		color: var(--pp-ink-2);
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
		margin-top: 10px;
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 96px;
		line-height: 0.85;
		letter-spacing: -4px;
		margin-top: auto;
	}
	.big small {
		margin-left: 6px;
		font-size: 28px;
		letter-spacing: 0;
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
		font-size: 15px;
		line-height: 1.45;
		opacity: 0.85;
	}
	.streak {
		display: flex;
		gap: 6px;
	}
	.streak span.on {
		background: var(--pp-ink);
	}
	.streak span {
		flex: 1;
		height: 10px;
		border-radius: 999px;
		background: rgba(22, 22, 26, 0.2);
	}
	.challenge {
		display: flex;
		flex-wrap: wrap;
		gap: 24px;
		height: 100%;
	}
	.ref-ph {
		flex: 0 0 200px;
		min-height: 180px;
		background: var(--pp-surface);
		border-radius: 20px;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
		font-size: 13px;
	}
	.row-btns {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.together {
		margin-top: var(--pp-gap);
	}
	.together-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
	}
	img.ref-ph {
		width: 200px;
		height: 180px;
		object-fit: contain;
	}
	.challenge-body {
		flex: 1 1 220px;
		display: flex;
		flex-direction: column;
		gap: 10px;
		align-items: flex-start;
	}
	.challenge-title {
		font-size: 30px;
		line-height: 1.05;
		font-weight: 700;
	}
	.section-title {
		font-size: 34px;
		font-weight: 700;
		letter-spacing: -1px;
		margin: 48px 0 16px;
	}
	.cats {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
		gap: var(--pp-gap);
	}
	.cat {
		container-type: inline-size;
		min-width: 0;
		min-height: 150px;
		padding: 20px;
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		text-decoration: none;
		transition: transform 0.25s var(--pp-spring);
	}
	.cat:hover {
		transform: translateY(-4px) rotate(-0.6deg);
		color: inherit;
	}
	.cat[aria-disabled='true'] {
		opacity: 0.6;
		pointer-events: none;
	}
	.cat.own {
		background: transparent;
		border: 2px dashed var(--pp-track);
	}
	.own-dot {
		background: conic-gradient(
			var(--pp-cat-anime),
			var(--pp-cat-real),
			var(--pp-cat-env),
			var(--pp-cat-anime)
		);
	}
	.cat.dark {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.dot {
		width: 14px;
		height: 14px;
		border-radius: 999px;
	}
	.cat-text {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.cat-name {
		font-family: var(--pp-font-display);
		font-weight: 700;
		/* scales with the card so long names (Fundamentals) never spill out */
		font-size: clamp(17px, 13cqi, 24px);
		overflow-wrap: anywhere;
	}
	.cat-blurb {
		font-size: 14px;
		color: var(--pp-muted);
	}
	.cat.dark .cat-blurb {
		color: var(--pp-on-ink-muted);
	}
	.step {
		width: 36px;
		height: 36px;
		display: grid;
		place-items: center;
		border-radius: 999px;
		background: var(--pp-accent-strong);
		color: #fff;
		font-weight: 700;
	}
	.step.light {
		background: var(--pp-yellow);
		color: var(--pp-ink);
	}
	h3 {
		font-size: 24px;
		font-weight: 700;
	}
	p {
		margin: 0;
		line-height: 1.55;
		color: var(--pp-ink-2);
	}
	.on-ink-p {
		color: var(--pp-on-ink-muted);
	}
</style>
