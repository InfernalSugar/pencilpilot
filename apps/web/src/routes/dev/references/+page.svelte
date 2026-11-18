<script lang="ts">
	import { enhance } from '$app/forms';
	import { TIERS, imageUrl } from '#lib/references.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Review references · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Dev only · ADR-0003</span>
	<h1>Review references</h1>
	<p class="lede">
		Approve only pictures that are safe for all ages, clearly not AI-made (check hands, eyes and
		text), not a franchise character, and good to draw from. Approved:
		{#each Object.entries(data.approved) as [c, n] (c)}<b>{c}</b> {n} ·
		{/each}
		waiting: <b>{data.candidates.length}</b>
	</p>
	{#if !data.candidates.length}
		<p>
			No candidates. Run <code>
				cargo run -p ref-importer -- fetch &lt;category&gt; "&lt;query&gt;"
			</code>.
		</p>
	{/if}
	<ul class="grid">
		{#each data.candidates as r (r.id)}
			<li class="card">
				<a href={r.pageUrl} target="_blank" rel="noreferrer" class="img">
					<img src={imageUrl(r)} alt={r.tags.join(', ')} loading="lazy" />
				</a>
				<div class="meta">
					<b>{r.author}</b> · {r.id} · “{r.query}” · difficulty {r.difficulty ?? '?'} (auto tier {r.tier ??
						'?'})
					<span class="tags">{r.tags.join(', ')}</span>
				</div>
				<form id="approve-{r.id}" method="POST" action="?/approve" use:enhance class="row">
					<input type="hidden" name="id" value={r.id} />
					<select name="category" aria-label="Category">
						{#each data.categories as c (c)}
							<option value={c} selected={c === r.category}>{c}</option>
						{/each}
					</select>
					<input name="title" placeholder="Title (optional)" aria-label="Title" />
					<button class="ok">Approve</button>
				</form>
				<div class="row small">
					<label>
						Topic
						<select name="topic" form="approve-{r.id}" aria-label="Topic">
							{#each data.topics[r.category] ?? [] as t (t.id)}
								<option value={t.id} selected={t.id === r.topic}>{t.name}</option>
							{/each}
						</select>
					</label>
					<label>
						Tier
						<select name="tier" form="approve-{r.id}" aria-label="Tier">
							{#each TIERS as t (t.id)}
								<option value={t.id} selected={t.id === r.tier}>{t.name}</option>
							{/each}
						</select>
					</label>
				</div>
				<form method="POST" action="?/reject" use:enhance>
					<input type="hidden" name="id" value={r.id} />
					<button class="no">Reject</button>
				</form>
			</li>
		{/each}
	</ul>
</div>

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 8px;
	}
	.lede {
		color: var(--pp-ink-2);
		max-width: 900px;
		line-height: 1.6;
	}
	.grid {
		list-style: none;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: 16px;
	}
	.card {
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.img img {
		width: 100%;
		height: 220px;
		object-fit: contain;
		background: var(--pp-ground);
		border-radius: 14px;
	}
	.meta {
		font-size: 13px;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.tags {
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		gap: 6px;
	}
	.small {
		font-size: 13px;
		gap: 12px;
	}
	.row input {
		flex: 1;
		min-width: 0;
	}
	select,
	input {
		padding: 8px;
		border-radius: 10px;
		border: 2px solid var(--pp-track);
		font: inherit;
		font-size: 14px;
	}
	button {
		padding: 8px 14px;
		border: 0;
		border-radius: 10px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.ok {
		background: var(--pp-ink);
		color: #fff;
	}
	.no {
		width: 100%;
		background: var(--pp-ground);
		color: var(--pp-accent-ink);
	}
</style>
