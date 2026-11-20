<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import {
		LESSONS,
		lessonsBySkill,
		recommendedLessons,
		skillLabel,
		type Recommendation
	} from '#lib/lessons.js';
	import { attempts } from '#lib/progress.js';

	const groups = lessonsBySkill();
	const total = LESSONS.reduce((n, l) => n + l.minutes, 0);

	let recs = $state<Recommendation[]>([]);
	onMount(() => {
		recs = recommendedLessons(attempts());
	});
	const recommended = $derived(new Set(recs.map((r) => r.lesson.id)));
</script>

<svelte:head><title>Lessons · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="pp-bento">
		<Tile span={3} as="section">
			<span class="pp-eyebrow">Lessons · {LESSONS.length} lessons · {total} min in all</span>
			<h1>Lessons</h1>
			<p class="lede">
				Short, practical lessons, each one a fix for something the grader spots. Read one in about
				three minutes, then lock it in with a drill.
			</p>
		</Tile>
		<Tile tone="yellow">
			<span class="pp-eyebrow ink">How it works</span>
			<p class="how">
				Draw, get feedback, and every tip links straight to the lesson that fixes it.
			</p>
			<div class="push"><Button href="/" variant="dark">Practice now</Button></div>
		</Tile>
	</div>

	{#if recs.length}
		<section aria-labelledby="recs-h">
			<h2 class="section" id="recs-h">Recommended for you</h2>
			<div class="grid">
				{#each recs as r (r.lesson.id)}
					<Tile
						tone="ink"
						href="/lessons/{r.lesson.id}?from={r.ruleId}"
						label="Recommended: {r.lesson.title}"
					>
						<span class="pp-eyebrow on-ink">{skillLabel(r.lesson.skill)}</span>
						<h3>{r.lesson.title}</h3>
						<p class="sum on-ink">{r.lesson.summary}</p>
						<span class="meta">
							<span class="badge">Came up in {r.count} of {r.of}</span>
							{r.lesson.minutes} min
						</span>
					</Tile>
				{/each}
			</div>
		</section>
	{/if}

	<div class="groups">
		{#each groups as g (g.id)}
			<section aria-labelledby="skill-{g.id}">
				<div class="group-head">
					<h2 class="section" id="skill-{g.id}">{g.label}</h2>
					<p class="blurb">{g.blurb}</p>
				</div>
				<div class="grid">
					{#each g.lessons as l (l.id)}
						<Tile href="/lessons/{l.id}" label={l.title}>
							<h3>{l.title}</h3>
							<p class="sum">{l.summary}</p>
							<span class="meta">
								{#if recommended.has(l.id)}<span class="badge">Recommended</span>{/if}
								{l.minutes} min read
							</span>
						</Tile>
					{/each}
				</div>
			</section>
		{/each}
	</div>
</div>

<style>
	h1 {
		font-size: clamp(44px, 6vw, 72px);
		line-height: 0.95;
	}
	h3 {
		font-size: 24px;
		line-height: 1.1;
		letter-spacing: -0.5px;
	}
	p {
		margin: 0;
	}
	.lede {
		font-size: 18px;
		line-height: 1.55;
		color: var(--pp-ink-2);
	}
	.how {
		font-weight: 500;
		line-height: 1.45;
	}
	.push {
		margin-top: auto;
	}
	.pp-eyebrow.ink {
		color: var(--pp-ink);
	}
	.pp-eyebrow.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.section {
		font-size: 22px;
		font-weight: 700;
		letter-spacing: -0.4px;
		margin: 32px 0 12px;
	}
	.group-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0 14px;
		margin: 32px 0 12px;
	}
	.group-head .section {
		margin: 0;
	}
	.blurb {
		color: var(--pp-muted);
		font-size: 15px;
	}
	.groups {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(100%, 520px), 1fr));
		column-gap: var(--pp-gap);
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(min(100%, 250px), 1fr));
		gap: var(--pp-gap);
	}
	.grid :global(.tile) {
		min-height: 190px;
	}
	.sum {
		font-size: 15px;
		line-height: 1.45;
		color: var(--pp-ink-2);
	}
	.sum.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.meta {
		margin-top: auto;
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.badge {
		padding: 4px 10px;
		border-radius: 999px;
		background: var(--pp-yellow);
		color: var(--pp-ink);
		font-size: 12px;
		font-weight: 700;
	}
</style>
