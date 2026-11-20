<script lang="ts">
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import { categories } from '../categories';
	import { TIERS } from '../references';
	import { CANVAS_PRESETS } from '@pencilpilot/paint/canvas-size';
	import { pickRefs } from './refs';
	import type { ClientMsg, LobbySettings, LobbyView } from './protocol';

	let { lobby, you, send }: { lobby: LobbyView; you: string; send: (m: ClientMsg) => void } =
		$props();

	const host = $derived(lobby.players.some((p) => p.id === you && p.host));
	const s = $derived(lobby.settings);
	let copied = $state(false);
	let starting = $state(false);
	const link = $derived(
		typeof location === 'undefined' ? '' : `${location.origin}/lobby/${lobby.code}`
	);

	const set = (patch: Partial<LobbySettings>) => send({ t: 'settings', settings: patch });

	async function copy() {
		try {
			await navigator.clipboard.writeText(link);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			/* clipboard blocked: the link is on screen to copy by hand */
		}
	}

	async function start() {
		starting = true;
		try {
			if (s.mode === 'free') send({ t: 'start' });
			else send({ t: 'start', refs: await pickRefs(s.category, s.tier, s.rounds) });
		} finally {
			starting = false;
		}
	}

	const TIMES = [30, 60, 120, 180, 300, 600];
	const fmtTime = (t: number) => (t < 60 ? `${t} s` : `${t / 60} min`);
</script>

<div class="pp-bento grid">
	<Tile span={2} tone="ink" as="section" label="Invite">
		<span class="pp-eyebrow on-ink">Invite friends</span>
		<span class="code" aria-label="Lobby code {lobby.code}">{lobby.code}</span>
		<p class="link">{link}</p>
		<div><Button onclick={copy}>{copied ? 'Copied ✓' : 'Copy invite link'}</Button></div>
	</Tile>

	<Tile span={2} as="section" label="Players">
		<h2>Players <span class="count">{lobby.players.length} / 8</span></h2>
		<ul class="players">
			{#each lobby.players as p (p.id)}
				<li class:away={!p.connected}>
					<span class="dot" aria-hidden="true"></span>
					<span class="name">{p.name}{p.id === you ? ' (you)' : ''}</span>
					{#if p.host}<span class="badge">Host</span>{/if}
					{#if !p.member}<span class="badge soft">Guest</span>{/if}
					{#if !p.connected}<span class="badge soft">Away</span>{/if}
					{#if host && p.id !== you}
						<button type="button" class="kick" onclick={() => send({ t: 'kick', playerId: p.id })}>
							Remove
						</button>
					{/if}
				</li>
			{/each}
		</ul>
	</Tile>

	<Tile span={4} as="section" label="Game settings">
		<h2>{host ? 'Set up the game' : 'Game settings'}</h2>
		<fieldset class="row" disabled={!host}>
			<legend>Mode</legend>
			<div class="seg" role="radiogroup" aria-label="Mode">
				<button
					type="button"
					role="radio"
					aria-checked={s.mode === 'competition'}
					onclick={() => set({ mode: 'competition' })}
				>
					Competition
				</button>
				<button
					type="button"
					role="radio"
					aria-checked={s.mode === 'free'}
					onclick={() => set({ mode: 'free' })}
				>
					Free draw
				</button>
			</div>
			<p class="help">
				{s.mode === 'competition'
					? 'Everyone draws the same picture against the clock. Then the drawings are shown one by one with who made them, and the grader’s highest score wins the round.'
					: 'Everyone draws together on one canvas and sees each other’s lines live. A reference is optional.'}
			</p>
		</fieldset>
		{#if s.mode === 'competition'}
			<div class="fields">
				<label>
					Rounds
					<select
						disabled={!host}
						value={s.rounds}
						onchange={(e) => set({ rounds: +e.currentTarget.value })}
					>
						{#each [1, 2, 3, 4, 5, 6, 8, 10] as n (n)}<option value={n}>{n}</option>{/each}
					</select>
				</label>
				<label>
					Time per round
					<select
						disabled={!host}
						value={s.timeLimit}
						onchange={(e) => set({ timeLimit: +e.currentTarget.value })}
					>
						{#each TIMES as t (t)}<option value={t}>{fmtTime(t)}</option>{/each}
					</select>
				</label>
				<label>
					Pictures
					<select
						disabled={!host}
						value={s.category}
						onchange={(e) => set({ category: e.currentTarget.value })}
					>
						{#each categories.filter((c) => c.id !== 'fundamentals') as c (c.id)}
							<option value={c.id}>{c.name}</option>
						{/each}
					</select>
				</label>
				<label>
					Difficulty
					<select
						disabled={!host}
						value={s.tier}
						onchange={(e) => set({ tier: +e.currentTarget.value })}
					>
						{#each TIERS as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
					</select>
				</label>
			</div>
		{/if}
		<div class="fields">
			<label>
				Canvas
				<select
					disabled={!host}
					value={`${s.width}x${s.height}`}
					onchange={(e) => {
						const [width, height] = e.currentTarget.value.split('x').map(Number);
						set({ width, height });
					}}
				>
					{#each CANVAS_PRESETS.filter((p) => p.width * p.height <= 1_000_000) as p (p.id)}
						<option value={`${p.width}x${p.height}`}>{p.label}</option>
					{/each}
				</select>
			</label>
		</div>
		{#if host}
			<div class="start">
				<Button onclick={start} disabled={starting}>
					{starting
						? 'Finding pictures…'
						: s.mode === 'free'
							? 'Start drawing together →'
							: 'Start the game →'}
				</Button>
			</div>
		{:else}
			<p class="help">Waiting for the host to start…</p>
		{/if}
	</Tile>
</div>

<style>
	.code {
		font: 800 56px var(--pp-font-display);
		letter-spacing: 6px;
	}
	.link {
		margin: 0;
		font-size: 13px;
		opacity: 0.8;
		word-break: break-all;
	}
	h2 {
		font-size: 22px;
	}
	.count {
		font: 600 14px var(--pp-font-body);
		color: var(--pp-muted);
	}
	.players {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.players li {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 40px;
	}
	.dot {
		width: 10px;
		height: 10px;
		border-radius: 999px;
		background: var(--pp-ok, #1f9d74);
	}
	.away .dot {
		background: var(--pp-track);
	}
	.name {
		font-weight: 600;
	}
	.badge {
		padding: 2px 8px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-size: 12px;
		font-weight: 600;
	}
	.badge.soft {
		background: var(--pp-surface-sunk);
		color: var(--pp-ink-2);
	}
	.kick {
		margin-left: auto;
		min-height: 32px;
		padding: 4px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	fieldset {
		border: 0;
		margin: 0;
		padding: 0;
	}
	legend {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
		margin-bottom: 6px;
	}
	.seg {
		display: inline-flex;
		gap: 2px;
		padding: 3px;
		border-radius: 999px;
		background: var(--pp-surface-sunk);
	}
	.seg button {
		min-height: 40px;
		padding: 6px 18px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: var(--pp-ink);
		font: 600 15px var(--pp-font-body);
		cursor: pointer;
	}
	.seg button[aria-checked='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	fieldset:disabled .seg button {
		cursor: default;
	}
	.help {
		margin: 8px 0 0;
		max-width: 70ch;
		color: var(--pp-ink-2);
	}
	.fields {
		display: flex;
		flex-wrap: wrap;
		gap: 12px 20px;
	}
	.fields label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.fields select {
		min-height: 40px;
		padding: 4px 10px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 500 15px var(--pp-font-body);
	}
	.start {
		margin-top: 4px;
	}
</style>
