<script lang="ts">
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import type { ClientMsg, LobbyView } from './protocol';

	let { lobby, you, send }: { lobby: LobbyView; you: string; send: (m: ClientMsg) => void } =
		$props();

	const host = $derived(lobby.players.some((p) => p.id === you && p.host));
	const round = $derived(lobby.results.at(-1));
	const fmt = (s: number | null) => (s === null ? '–' : s.toFixed(1));
	const name = (id: string | null) => lobby.players.find((p) => p.id === id)?.name ?? 'Nobody';
</script>

{#if lobby.phase === 'reveal' && lobby.reveal}
	{@const e = lobby.reveal.entry}
	<section class="reveal" aria-label="Reveal">
		<span class="pp-eyebrow">
			Round {lobby.round} · drawing {lobby.reveal.index + 1} of {lobby.reveal.total}
		</span>
		<h1>{e.name}{e.playerId === you ? ' (you)' : ''}</h1>
		<div class="pics">
			{#if lobby.reference}
				<figure>
					<img src={lobby.reference.imageUrl} alt="Reference: {lobby.reference.title}" />
					<figcaption>Reference</figcaption>
				</figure>
			{/if}
			<figure>
				{#if e.png}
					<img class="drawing" src={e.png} alt="Drawing by {e.name}" />
				{:else}
					<div class="empty">Ran out of time with nothing drawn</div>
				{/if}
				<figcaption>By {e.name}</figcaption>
			</figure>
		</div>
		<div class="row">
			{#if e.playerId !== you && e.png}
				<button
					type="button"
					class="react"
					onclick={() => send({ t: 'react', target: e.playerId, v: 1 })}
				>
					👍 <b>{e.likes}</b>
				</button>
				<button
					type="button"
					class="react"
					onclick={() => send({ t: 'react', target: e.playerId, v: -1 })}
				>
					👎 <b>{e.dislikes}</b>
				</button>
			{:else}
				<span class="counts">👍 {e.likes} · 👎 {e.dislikes}</span>
			{/if}
			<span class="grow"></span>
			{#if host}
				<Button onclick={() => send({ t: 'next' })}>
					{lobby.reveal.index + 1 < lobby.reveal.total ? 'Next drawing →' : 'See the scores →'}
				</Button>
			{:else}
				<span class="wait">The host moves on when everyone’s had a look.</span>
			{/if}
		</div>
	</section>
{:else if lobby.phase === 'round-end' && round}
	<section aria-label="Round results">
		<span class="pp-eyebrow">Round {round.round} of {lobby.settings.rounds}</span>
		<h1>{round.winnerId ? `${name(round.winnerId)} wins the round!` : 'No winner this round'}</h1>
		<p class="lede">
			The grader decides: the drawing closest to the picture wins. Likes are just for fun.
		</p>
		{@render table(
			round.entries
				.slice()
				.sort((a, b) => (b.score ?? -1) - (a.score ?? -1))
				.map((x) => ({
					id: x.playerId,
					name: x.name,
					a: fmt(x.score),
					b: `👍 ${x.likes} · 👎 ${x.dislikes}`
				})),
			'Score',
			'Reactions',
			round.winnerId
		)}
		{#if lobby.totals.length && lobby.settings.rounds > 1}
			<h2>Game so far</h2>
			{@render table(
				lobby.totals.map((t) => ({
					id: t.playerId,
					name: t.name,
					a: t.total.toFixed(1),
					b: `${t.wins}`
				})),
				'Total',
				'Round wins',
				null
			)}
		{/if}
		<div class="row">
			{#if host}
				<Button onclick={() => send({ t: 'next' })}>
					{lobby.round < lobby.settings.rounds
						? `Start round ${lobby.round + 1} →`
						: 'Final results →'}
				</Button>
			{:else}
				<span class="wait">Waiting for the host…</span>
			{/if}
		</div>
	</section>
{:else if lobby.phase === 'final'}
	<section aria-label="Final results">
		<span class="pp-eyebrow">Game over</span>
		<h1>{lobby.winnerId ? `${name(lobby.winnerId)} wins!` : 'Game over'}</h1>
		<p class="lede">
			Highest total score over {lobby.settings.rounds} round{lobby.settings.rounds > 1 ? 's' : ''};
			ties go to more round wins.
		</p>
		{@render table(
			lobby.totals.map((t) => ({
				id: t.playerId,
				name: t.name,
				a: t.total.toFixed(1),
				b: `${t.wins}`
			})),
			'Total',
			'Round wins',
			lobby.winnerId
		)}
		<div class="row">
			{#if host}
				<Button onclick={() => send({ t: 'again' })}>Play again →</Button>
			{:else}
				<span class="wait">The host can start another game. Stay here!</span>
			{/if}
		</div>
	</section>
{/if}

{#snippet table(
	rows: { id: string; name: string; a: string; b: string }[],
	a: string,
	b: string,
	winner: string | null
)}
	<Tile as="section">
		<table>
			<thead><tr><th>Player</th><th>{a}</th><th>{b}</th></tr></thead>
			<tbody>
				{#each rows as r (r.id)}
					<tr class:win={r.id === winner}>
						<td>{r.name}{r.id === you ? ' (you)' : ''}{r.id === winner ? ' · Winner' : ''}</td>
						<td class="num">{r.a}</td>
						<td>{r.b}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</Tile>
{/snippet}

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
		margin-bottom: 12px;
	}
	h2 {
		font-size: 22px;
		margin: var(--pp-gap) 0 8px;
	}
	.lede {
		margin: 0 0 var(--pp-gap);
		color: var(--pp-ink-2);
	}
	.pics {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
		gap: var(--pp-gap);
	}
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	figure img,
	.empty {
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		border-radius: 20px;
		background: var(--pp-surface);
	}
	.drawing {
		background: #fff;
		border: 1px solid var(--pp-track);
	}
	.empty {
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
	}
	figcaption {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		margin-top: var(--pp-gap);
	}
	.grow {
		flex: 1;
	}
	.react {
		min-height: 48px;
		padding: 8px 18px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 18px var(--pp-font-body);
		cursor: pointer;
	}
	.counts,
	.wait {
		color: var(--pp-ink-2);
		font-weight: 600;
	}
	table {
		width: 100%;
		border-collapse: collapse;
	}
	th,
	td {
		padding: 10px 8px;
		text-align: left;
		border-bottom: 1px solid var(--pp-track);
	}
	th {
		font-size: 13px;
		color: var(--pp-muted);
	}
	.num {
		font: 800 20px var(--pp-font-display);
	}
	tr.win td {
		background: var(--pp-yellow);
		color: var(--pp-ink);
	}
</style>
