<script lang="ts">
	import { enhance } from '$app/forms';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import CommunityNav from './CommunityNav.svelte';
	import { imageUrl, titleOf } from '#lib/references.js';
	import type { loadView } from '../server/community-pages';

	let {
		data,
		form
	}: {
		data: ReturnType<typeof loadView>;
		form: { message?: string; reported?: boolean; ok?: boolean } | null | undefined;
	} = $props();
	let reporting = $state('');

	const img = (id: string) => `/community/img/${id}`;
	const ref = $derived(data.signedIn ? (data.pair?.a.reference ?? data.toRate?.reference) : null);
	/** after any judgement, close the report picker */
	const done = () => {
		reporting = '';
		return async ({ update }: { update: () => Promise<void> }) => update();
	};
</script>

<svelte:head><title>Community · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Community · beta</span>
	<h1>Community</h1>
	<p class="lede">
		Real people judging real drawings. Only signed-in students see shared drawings, and never your
		name.
	</p>
	<CommunityNav current={data.view} />

	{#if form?.message}<p class="msg" role="alert">{form.message}</p>{/if}
	{#if form && 'reported' in form}
		<p class="msg ok" role="status">Thanks. Drawings with several reports are hidden.</p>
	{/if}

	{#if !data.signedIn}
		<Tile tone="lilac">
			<h2>Sign in to join in</h2>
			<p>
				Compare drawings, rate them, and share your own for a human opinion. It’s free, and your
				practice history comes with you.
			</p>
			<div class="row">
				<Button href="/signup?next=/community">Create account</Button>
				<Button href="/login?next=/community" variant="outline">Sign in</Button>
			</div>
		</Tile>
	{:else if data.view === 'closer'}
		{#if data.pair && ref}
			{@const pair = data.pair}
			<p class="ask">Which drawing is closer to the reference?</p>
			<div class="compare">
				<Tile as="section" label="Reference">
					<h2>Reference</h2>
					<img class="pic" src={imageUrl(ref)} alt="Reference: {titleOf(ref)}" />
					<p class="credit">
						By <a href={ref.authorUrl} target="_blank" rel="noreferrer">{ref.author}</a> via Pixabay
					</p>
				</Tile>
				{#each [pair.a, pair.b] as d, i (d.id)}
					<Tile as="section" label="Drawing {i ? 'B' : 'A'}">
						<h2>Drawing {i ? 'B' : 'A'}</h2>
						<img class="pic drawing" src={img(d.id)} alt="Drawing {i ? 'B' : 'A'}" />
						<form method="POST" action="?/closer" use:enhance={done}>
							<input type="hidden" name="a" value={pair.a.id} />
							<input type="hidden" name="b" value={pair.b.id} />
							<input type="hidden" name="winner" value={d.id} />
							<Button type="submit">{i ? 'B' : 'A'} is closer</Button>
						</form>
						{@render report(d.id)}
					</Tile>
				{/each}
			</div>
			<p class="hint">Not sure? Go with your gut. Lots of quick votes beat a few careful ones.</p>
		{:else}
			{@render empty()}
		{/if}
	{:else if data.toRate && ref}
		{@const d = data.toRate}
		<div class="compare two">
			<Tile as="section" label="Reference">
				<h2>Reference</h2>
				<img class="pic" src={imageUrl(ref)} alt="Reference: {titleOf(ref)}" />
			</Tile>
			<Tile as="section" label="Their drawing">
				<h2>Their drawing</h2>
				<img class="pic drawing" src={img(d.id)} alt="The drawing to rate" />
				{@render report(d.id)}
			</Tile>
		</div>
		{#key d.id}
			<Tile as="section" label="Rubric">
				<form class="rubric" method="POST" action="?/rate" use:enhance={done}>
					<input type="hidden" name="drawing" value={d.id} />
					{#each data.rubric as r (r.id)}
						<div class="row-item" role="radiogroup" aria-labelledby="rub-{r.id}">
							<p class="legend" id="rub-{r.id}"><b>{r.label}</b> <span>{r.hint}</span></p>
							<div class="scale">
								{#each [1, 2, 3, 4, 5] as n (n)}
									<label>
										<input type="radio" name={r.id} value={n} required />
										<span>{n}</span>
									</label>
								{/each}
							</div>
						</div>
					{/each}
					<p class="hint">1 = far off · 3 = fair · 5 = spot on</p>
					<div><Button type="submit">Send rating</Button></div>
				</form>
			</Tile>
		{/key}
	{:else}
		{@render empty()}
	{/if}
</div>

{#snippet report(id: string)}
	{#if reporting === id}
		<form class="report" method="POST" action="?/report" use:enhance={done}>
			<input type="hidden" name="drawing" value={id} />
			<span>What’s wrong?</span>
			<button name="reason" value="unsafe">Not appropriate</button>
			<button name="reason" value="not_a_drawing">Not a drawing</button>
			<button type="button" onclick={() => (reporting = '')}>Cancel</button>
		</form>
	{:else}
		<button type="button" class="link" onclick={() => (reporting = id)}>Report</button>
	{/if}
{/snippet}

{#snippet empty()}
	<Tile tone="yellow">
		<h2>You’re all caught up</h2>
		<p>
			There’s nothing new to judge right now. Share your own drawings from the results page to get
			more going.
		</p>
		<div class="row"><Button href="/" variant="dark">Draw something →</Button></div>
	</Tile>
{/snippet}

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
	}
	h2 {
		font-size: 20px;
		letter-spacing: -0.3px;
	}
	.lede {
		margin: 6px 0 0;
		color: var(--pp-ink-2);
		max-width: 60ch;
	}
	.ask {
		font: 700 22px var(--pp-font-display);
		margin: 0 0 12px;
	}
	.compare {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: var(--pp-gap);
		margin-bottom: var(--pp-gap);
	}
	.compare.two {
		grid-template-columns: repeat(2, minmax(0, 1fr));
	}
	@media (max-width: 760px) {
		.compare,
		.compare.two {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.pic {
		display: block;
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		border-radius: 16px;
		background: var(--pp-surface-sunk);
	}
	.pic.drawing {
		background: #fff;
		border: 1px solid var(--pp-track);
	}
	.credit,
	.hint {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
	}
	.msg {
		padding: 12px 16px;
		border-radius: 14px;
		background: var(--pp-surface-sunk);
		font-weight: 600;
	}
	.rubric {
		display: flex;
		flex-direction: column;
		gap: 14px;
	}
	.row-item {
		display: flex;
		flex-wrap: wrap;
		gap: 8px 16px;
		align-items: center;
		justify-content: space-between;
	}
	.legend {
		margin: 0;
		display: flex;
		flex-direction: column;
	}
	.legend span {
		font-size: 13px;
		color: var(--pp-muted);
	}
	.scale {
		display: flex;
		gap: 6px;
	}
	.scale label {
		position: relative;
	}
	.scale input {
		position: absolute;
		opacity: 0;
		inset: 0;
		margin: 0;
		cursor: pointer;
	}
	.scale span {
		display: grid;
		place-items: center;
		width: 44px;
		height: 44px;
		border-radius: 999px;
		background: var(--pp-surface-sunk);
		font-weight: 700;
	}
	.scale input:checked + span {
		background: var(--pp-ink);
		color: #fff;
	}
	.scale input:focus-visible + span {
		outline: 3px solid var(--pp-accent);
		outline-offset: 2px;
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
	.report {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		align-items: center;
		font-size: 13px;
	}
	.report button {
		padding: 6px 10px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface-sunk);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
</style>
