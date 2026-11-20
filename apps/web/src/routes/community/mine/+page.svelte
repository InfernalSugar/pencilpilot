<script lang="ts">
	import { enhance } from '$app/forms';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import CommunityNav from '#lib/community/CommunityNav.svelte';
	import { imageUrl, titleOf } from '#lib/references.js';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let confirm = $state('');

	const pct = (wins: number, n: number) => Math.round((wins / n) * 100);
	const one = (v: number) => v.toFixed(1);
</script>

<svelte:head><title>My shared drawings · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Community · beta</span>
	<h1>My shared drawings</h1>
	<CommunityNav current="mine" />

	{#if form?.message}<p class="msg" role="alert">{form.message}</p>{/if}
	{#if form && 'deleted' in form}<p class="msg" role="status">Deleted.</p>{/if}

	{#if !data.signedIn}
		<Tile tone="lilac">
			<h2>Sign in to see what people think</h2>
			<div class="row"><Button href="/login?next=/community/mine">Sign in</Button></div>
		</Tile>
	{:else if !data.drawings.length}
		<Tile tone="yellow">
			<h2>Nothing shared yet</h2>
			<p>
				After you draw a reference, press <b>Share with the community</b> on the results page. Other students
				will compare it with drawings of the same picture and rate it.
			</p>
			<div class="row"><Button href="/" variant="dark">Draw something →</Button></div>
		</Tile>
	{:else}
		<ul class="list">
			{#each data.drawings as d (d.id)}
				<li>
					<Tile as="article" label={titleOf(d.reference)}>
						<div class="pics">
							<img src={imageUrl(d.reference)} alt="Reference: {titleOf(d.reference)}" />
							<img class="drawing" src="/community/img/{d.id}" alt="Your drawing" />
						</div>
						<h2>{titleOf(d.reference)}</h2>
						{#if d.hidden}
							<p class="warn">Hidden after reports from other students.</p>
						{/if}
						<dl>
							<div>
								<dt>Grader score</dt>
								<dd>{d.score == null ? '–' : one(d.score)}</dd>
							</div>
							<div>
								<dt>Picked as closer</dt>
								<dd>
									{d.comparisons ? `${pct(d.wins, d.comparisons)}%` : '–'}
									<small>of {d.comparisons} vote{d.comparisons === 1 ? '' : 's'}</small>
								</dd>
							</div>
							{#if d.rubric}
								{#each data.rubric as r (r.id)}
									<div>
										<dt>{r.label}</dt>
										<dd>{one(d.rubric[r.id])} <small>/ 5</small></dd>
									</div>
								{/each}
							{:else}
								<div>
									<dt>Ratings</dt>
									<dd>– <small>none yet</small></dd>
								</div>
							{/if}
						</dl>
						{#if confirm === d.id}
							<form method="POST" action="?/delete" use:enhance class="row">
								<input type="hidden" name="drawing" value={d.id} />
								<Button type="submit" variant="dark">Delete for good</Button>
								<Button type="button" variant="outline" onclick={() => (confirm = '')}>Keep</Button>
							</form>
						{:else}
							<button type="button" class="link" onclick={() => (confirm = d.id)}>Delete</button>
						{/if}
					</Tile>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
	}
	h2 {
		font-size: 18px;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: var(--pp-gap);
	}
	.pics {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.pics img {
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
	}
	.pics .drawing {
		background: #fff;
		border: 1px solid var(--pp-track);
	}
	dl {
		margin: 0;
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px 12px;
	}
	dt {
		font-size: 12px;
		color: var(--pp-muted);
		font-weight: 600;
	}
	dd {
		margin: 0;
		font: 700 18px var(--pp-font-display);
	}
	small {
		font: 500 12px var(--pp-font-body);
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
	}
	.msg,
	.warn {
		padding: 10px 14px;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		font-weight: 600;
	}
	.link {
		align-self: flex-start;
		border: 0;
		background: none;
		padding: 4px 0;
		font: 600 13px var(--pp-font-body);
		color: var(--pp-muted);
		text-decoration: underline;
		cursor: pointer;
	}
</style>
