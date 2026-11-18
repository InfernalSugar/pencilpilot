<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import {
		attempts,
		averageScore,
		bestScore,
		dailyScores,
		recurringMistakes,
		skillAverages,
		streakDays,
		thumbs,
		type Attempt
	} from '#lib/progress.js';
	import { ruleById } from '#lib/rules.js';
	import { humanize } from '#lib/grader.js';
	import { SYNCED } from '#lib/sync.js';

	interface DrillResult {
		id: string;
		at: number;
		avg: number;
		rounds: number[];
	}

	let history = $state<Attempt[]>([]);
	let drills = $state<DrillResult[]>([]);
	let pics = $state<Record<string, string>>({});
	let range = $state<'week' | 'month' | 'all'>('month');
	let loaded = $state(false);

	function load() {
		history = attempts();
		pics = thumbs();
		try {
			drills = JSON.parse(localStorage.getItem('pp.drills') ?? '[]');
		} catch {
			drills = [];
		}
		loaded = true;
	}
	onMount(() => {
		load();
		// a signed-in browser may receive synced history a moment later
		window.addEventListener(SYNCED, load);
		return () => window.removeEventListener(SYNCED, load);
	});

	const avg30 = $derived(averageScore(history, 30));
	const best = $derived(bestScore(history));
	const streak = $derived(streakDays(history));
	const series = $derived(
		dailyScores(history, range === 'week' ? 7 : range === 'month' ? 30 : null)
	);
	const skills = $derived(skillAverages(history));
	const patterns = $derived(recurringMistakes(history, 10, 3));
	const recent = $derived([...history].reverse().slice(0, 10));

	const AXES = [
		{ key: 'outline', label: 'Outlines' },
		{ key: 'placement', label: 'Placement' },
		{ key: 'silhouette', label: 'Shapes' },
		{ key: 'tone', label: 'Light' },
		{ key: 'line_confidence', label: 'Lines' }
	];
	const radar = $derived(
		AXES.map((a, i) => {
			const ang = -Math.PI / 2 + (i * 2 * Math.PI) / AXES.length;
			const v = skills[a.key];
			return { ...a, ang, v, has: v !== undefined };
		})
	);
	const pt = (ang: number, r: number) =>
		`${(110 + Math.cos(ang) * r).toFixed(1)},${(110 + Math.sin(ang) * r).toFixed(1)}`;
	const ranked = $derived(radar.filter((a) => a.has).sort((a, b) => b.v - a.v));

	/** Chart geometry: x by day index, y by score 0–10. */
	const W = 820;
	const H = 220;
	const chart = $derived.by(() => {
		if (!series.length) return null;
		const n = series.length;
		const x = (i: number) => (n === 1 ? W / 2 : 20 + (i * (W - 40)) / (n - 1));
		const y = (s: number) => H - 20 - (s / 10) * (H - 40);
		const pts = series.map((d, i) => [x(i), y(d.avg)] as const);
		const line = pts
			.map(([px, py], i) => `${i ? 'L' : 'M'}${px.toFixed(1)} ${py.toFixed(1)}`)
			.join(' ');
		const area = `${line} L${pts.at(-1)![0].toFixed(1)} ${H} L${pts[0][0].toFixed(1)} ${H} Z`;
		return { pts, line, area, y };
	});
	const fmtDay = (t: number) =>
		new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
</script>

<svelte:head><title>Progress · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="top">
		<span class="pp-eyebrow">Your practice</span>
		<h1>Progress</h1>
	</div>

	{#if loaded && !history.length && !drills.length}
		<div class="pp-bento">
			<Tile span={3}>
				<h2>No drawings yet</h2>
				<p>Draw a reference and your scores, skills and habits show up here.</p>
				<div><Button href="/practice/animals">Start with Animals →</Button></div>
			</Tile>
			<Tile tone="yellow">
				<span class="pp-eyebrow ink">Tip</span>
				<p>Progress is saved in this browser. Accounts arrive later.</p>
			</Tile>
		</div>
	{:else if loaded}
		<div class="pp-bento stats">
			<Tile tone="ink">
				<span class="pp-eyebrow on-ink">30-day average</span>
				<span class="big">{avg30 === null ? '–' : avg30.toFixed(1)}</span>
			</Tile>
			<Tile tone="yellow">
				<span class="pp-eyebrow ink">Streak</span>
				<span class="big">{streak}<small> {streak === 1 ? 'day' : 'days'}</small></span>
			</Tile>
			<Tile>
				<span class="pp-eyebrow">Drawings</span>
				<span class="big">{history.length}</span>
			</Tile>
			<Tile tone="lilac">
				<span class="pp-eyebrow lilac">Personal best</span>
				<span class="big">{best === null ? '–' : best.toFixed(1)}</span>
			</Tile>
		</div>

		<div class="pp-bento main">
			<Tile span={3} as="section" label="Score over time">
				<div class="head">
					<h2>Score over time</h2>
					<div class="seg" role="radiogroup" aria-label="Time range">
						{#each [['week', 'Week'], ['month', 'Month'], ['all', 'All']] as [id, name] (id)}
							<button
								type="button"
								role="radio"
								aria-checked={range === id}
								onclick={() => (range = id as typeof range)}
							>
								{name}
							</button>
						{/each}
					</div>
				</div>
				{#if chart}
					<svg
						class="chart"
						viewBox="0 0 {W} {H}"
						role="img"
						aria-label="Daily average score, {series.length} days, latest {series
							.at(-1)!
							.avg.toFixed(1)}"
					>
						{#each [0, 5, 10] as s (s)}
							<line x1="0" x2={W} y1={chart.y(s)} y2={chart.y(s)} class="gridline" />
							<text x="4" y={chart.y(s) - 4} class="axis">{s}</text>
						{/each}
						<path d={chart.area} class="area" />
						<path d={chart.line} class="line" />
						{#each chart.pts as [px, py], i (i)}
							<circle cx={px} cy={py} r="5" class="dot">
								<title>
									{fmtDay(series[i].day)}: {series[i].avg.toFixed(1)} ({series[i].n}
									drawing{series[i].n === 1 ? '' : 's'})
								</title>
							</circle>
						{/each}
					</svg>
				{:else}
					<p class="muted">No drawings in this period.</p>
				{/if}
			</Tile>

			<Tile rows={2} as="section" label="Skills">
				<h2>Skills</h2>
				<svg class="radar" viewBox="-30 0 280 220" role="img" aria-label="Skills radar">
					{#each [1, 0.66, 0.33] as k (k)}
						<polygon points={radar.map((a) => pt(a.ang, 80 * k)).join(' ')} class="ring" />
					{/each}
					{#if ranked.length >= 3}
						<polygon
							points={radar.map((a) => pt(a.ang, 80 * (a.v ?? 0))).join(' ')}
							class="shape"
						/>
					{/if}
					{#each radar as a (a.key)}
						<text
							x={110 + Math.cos(a.ang) * 100}
							y={110 + Math.sin(a.ang) * 100 + 4}
							class="axis"
							text-anchor="middle"
						>
							{a.label}
						</text>
					{/each}
				</svg>
				{#if ranked.length}
					<dl class="sk">
						<dt>Strongest</dt>
						<dd>{ranked[0].label} · {Math.round(ranked[0].v * 100)}%</dd>
						<dt>Needs work</dt>
						<dd class="weak">{ranked.at(-1)!.label} · {Math.round(ranked.at(-1)!.v * 100)}%</dd>
					</dl>
				{:else}
					<p class="muted">Draw a few references to see your skills.</p>
				{/if}
			</Tile>

			<Tile span={3} as="section" label="Recent drawings">
				<div class="head">
					<h2>Recent drawings</h2>
				</div>
				<ul class="recent">
					{#each recent as a (a.at)}
						<li>
							<div class="thumb">
								{#if pics[a.at]}<img
										src={pics[a.at]}
										alt="Your drawing: {a.title ?? a.category}"
									/>{/if}
								<span class="badge">{a.score.toFixed(1)}</span>
							</div>
							<span class="cap">{a.title ?? humanize(a.category)}</span>
						</li>
					{/each}
				</ul>
			</Tile>
		</div>

		{#if patterns.length}
			<h2 class="section">Habits to work on</h2>
			<div class="patterns">
				{#each patterns as p (p.ruleId)}
					{@const r = ruleById(p.ruleId)}
					{#if r}
						<Tile tone="lilac">
							<span class="pp-eyebrow lilac">{p.count} of your last {p.window} drawings</span>
							<span class="mid">{r.title}</span>
							<div class="acts">
								{#if r.lesson}<Button href="/lessons/{r.lesson}?from={r.id}" variant="dark">
										{humanize(r.lesson)}
									</Button>{/if}
								{#if r.drill}<Button href="/drills/{r.drill}" variant="outline">Drill</Button>{/if}
							</div>
						</Tile>
					{/if}
				{/each}
			</div>
		{/if}

		{#if drills.length}
			<h2 class="section">Drills</h2>
			<div class="patterns">
				{#each [...drills].reverse().slice(0, 6) as d (d.at)}
					<Tile>
						<span class="pp-eyebrow">{new Date(d.at).toLocaleDateString()}</span>
						<span class="mid">{humanize(d.id)}</span>
						<span class="note">Average {d.avg.toFixed(1)} over {d.rounds.length} rounds</span>
					</Tile>
				{/each}
			</div>
		{/if}
	{/if}
</div>

<style>
	.top {
		margin-bottom: 16px;
	}
	h1 {
		font-size: clamp(40px, 6vw, 64px);
		line-height: 0.95;
		margin-top: 4px;
	}
	h2 {
		font-size: 22px;
		font-weight: 700;
		letter-spacing: -0.4px;
	}
	.main,
	.patterns {
		margin-top: var(--pp-gap);
	}
	.patterns {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: var(--pp-gap);
	}
	.section {
		margin: 32px 0 0;
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 72px;
		line-height: 0.85;
		letter-spacing: -3px;
		margin-top: auto;
	}
	.big small {
		margin-left: 6px;
		font-size: 24px;
		letter-spacing: 0;
	}
	.mid {
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 24px;
		line-height: 1.1;
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
	.note,
	.muted {
		font-size: 14px;
		color: var(--pp-muted);
		margin: 0;
	}
	.head {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
		flex-wrap: wrap;
	}
	.seg {
		display: flex;
		gap: 4px;
		padding: 4px;
		background: var(--pp-ground);
		border-radius: 999px;
	}
	.seg button {
		padding: 8px 14px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
		color: var(--pp-ink);
	}
	.seg button[aria-checked='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.chart {
		width: 100%;
		height: auto;
	}
	.gridline {
		stroke: var(--pp-track);
	}
	.axis {
		font: 600 11px var(--pp-font-body);
		fill: var(--pp-muted);
	}
	.area {
		fill: var(--pp-accent);
		opacity: 0.1;
	}
	.line {
		fill: none;
		stroke: var(--pp-accent);
		stroke-width: 4;
		stroke-linejoin: round;
		stroke-linecap: round;
	}
	.dot {
		fill: var(--pp-accent);
	}
	.radar {
		width: 100%;
		max-width: 260px;
		align-self: center;
	}
	.ring {
		fill: none;
		stroke: var(--pp-track);
		stroke-width: 1.5;
	}
	.shape {
		fill: var(--pp-accent);
		fill-opacity: 0.18;
		stroke: var(--pp-accent);
		stroke-width: 3;
		stroke-linejoin: round;
	}
	.sk {
		margin: 0;
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 6px 12px;
		font-size: 14px;
	}
	.sk dd {
		margin: 0;
		font-weight: 600;
		text-align: right;
	}
	.sk .weak {
		color: var(--pp-accent-ink);
	}
	.recent {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
		gap: 12px;
	}
	.recent li {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
	}
	.thumb {
		position: relative;
		aspect-ratio: 4 / 3;
		background: var(--pp-surface-sunk);
		border-radius: 14px;
		overflow: hidden;
	}
	.thumb img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.badge {
		position: absolute;
		left: 8px;
		bottom: 8px;
		padding: 3px 9px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 600;
		font-size: 12px;
	}
	.cap {
		font-size: 12px;
		color: var(--pp-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.acts {
		margin-top: auto;
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
	}
	p {
		margin: 0;
	}
</style>
