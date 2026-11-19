<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '../ui/Tile.svelte';
	import Button from '../ui/Button.svelte';
	import PaintCanvas, { BLANK_STATUS, type CanvasStatus } from '../paint/PaintCanvas.svelte';
	import RefViewer from '../paint/RefViewer.svelte';
	import MiniToolbar from './MiniToolbar.svelte';
	import { DEFAULT_BRUSH } from '../paint/brushes';
	import { gradeImages, toGray, type GradeCategory } from '../grader';
	import { referencePixels } from '../references';
	import { clockOf } from '../timed';
	import { asReference } from './refs';
	import type { ClientMsg, LobbyView } from './protocol';

	let {
		lobby,
		you,
		send,
		left
	}: {
		lobby: LobbyView;
		you: string;
		send: (m: ClientMsg) => void;
		/** ms left until a server time */
		left: (t: number) => number;
	} = $props();

	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let brushId = $state(DEFAULT_BRUSH);
	let color = $state('#16161a');
	let size = $state(1);
	let opacity = $state(1);
	let eraser = $state(false);
	let handedIn = $state(false);
	let grading = $state(false);
	let remaining = $state(0);

	const done = $derived(lobby.submitted.includes(you) || handedIn);
	const waitingFor = $derived(
		lobby.players.filter((p) => p.connected && !lobby.submitted.includes(p.id))
	);

	onMount(() => {
		const tick = () => (remaining = lobby.endsAt ? Math.max(0, left(lobby.endsAt)) : 0);
		tick();
		const id = setInterval(tick, 250);
		return () => clearInterval(id);
	});

	/** Grade here (like everywhere else on the site) and hand the drawing in. */
	export async function submitNow() {
		if (done || grading) return;
		grading = true;
		handedIn = true;
		const d = canvas?.exportDrawing();
		try {
			const ref = lobby.reference;
			if (!d || status.blank || !ref) return send({ t: 'submit', png: null, score: null });
			const refData = await referencePixels(asReference(ref), d.width, d.height);
			const cat = ref.category === 'cartoon' ? 'anime' : ref.category;
			const g = await gradeImages(d.width, d.height, d.gray, toGray(refData.data), {
				category: cat as GradeCategory,
				strokes: d.strokes
			});
			send({ t: 'submit', png: d.png, score: g.score });
		} catch {
			// couldn't grade (e.g. the picture didn't load): still show the drawing, unscored
			send({ t: 'submit', png: d && !status.blank ? d.png : null, score: null });
		} finally {
			grading = false;
		}
	}
</script>

<div class="round">
	<div class="top">
		<span class="pp-eyebrow">Round {lobby.round} of {lobby.settings.rounds}</span>
		<span
			class="timer"
			class:urgent={remaining <= 10_000}
			role="timer"
			aria-label="Time left {clockOf(remaining / 1000)}"
		>
			{lobby.phase === 'collecting' ? 'Time’s up!' : clockOf(remaining / 1000)}
		</span>
	</div>
	<div class="layout">
		<div class="side">
			{#if lobby.reference}
				<Tile as="section" label="Reference">
					<h2>{lobby.reference.title}</h2>
					{#key lobby.reference.id}
						<RefViewer src={lobby.reference.imageUrl} alt="Reference: {lobby.reference.title}" />
					{/key}
					<p class="credit">
						By <a href={lobby.reference.authorUrl} target="_blank" rel="noreferrer">
							{lobby.reference.author}
						</a> via Pixabay
					</p>
				</Tile>
			{/if}
			<Tile as="section" label="Handed in">
				<h2>
					Handed in {lobby.submitted.length} / {lobby.players.filter((p) => p.connected).length}
				</h2>
				{#if waitingFor.length}
					<p class="credit">Still drawing: {waitingFor.map((p) => p.name).join(', ')}</p>
				{/if}
				{#if !done}
					<Button variant="dark" onclick={submitNow} disabled={status.blank}>I’m done →</Button>
				{/if}
			</Tile>
		</div>
		<div class="main">
			<div class="canvas-tile">
				{#key lobby.round}
					<PaintCanvas
						bind:this={canvas}
						width={lobby.settings.width}
						height={lobby.settings.height}
						{brushId}
						{eraser}
						{size}
						{opacity}
						{color}
						locked={done || lobby.phase === 'collecting'}
						onchange={(s) => (status = s)}
						maxHeight="70vh"
					/>
				{/key}
				{#if done}
					<div class="cover" role="status">
						<b>{grading ? 'Grading…' : 'Handed in!'}</b>
						<span>Waiting for the others. Nobody sees your drawing until the reveal.</span>
					</div>
				{/if}
			</div>
			<MiniToolbar
				bind:brushId
				bind:color
				bind:size
				bind:opacity
				bind:eraser
				canUndo={status.canUndo && !done}
				onundo={() => canvas?.undo()}
				onclear={() => canvas?.clear()}
			/>
		</div>
	</div>
</div>

<style>
	.top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		margin-bottom: 12px;
	}
	.timer {
		min-width: 110px;
		padding: 10px 18px;
		border-radius: var(--pp-radius-btn);
		background: var(--pp-yellow);
		color: var(--pp-ink);
		font: 700 24px var(--pp-font-display);
		text-align: center;
		font-variant-numeric: tabular-nums;
	}
	.timer.urgent {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.layout {
		display: grid;
		grid-template-columns: 320px minmax(0, 1fr);
		gap: var(--pp-gap);
		align-items: start;
	}
	@media (max-width: 960px) {
		.layout {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.side,
	.main {
		display: flex;
		flex-direction: column;
		gap: var(--pp-gap);
		min-width: 0;
	}
	h2 {
		font-size: 18px;
	}
	.credit {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.canvas-tile {
		position: relative;
		border-radius: var(--pp-radius-tile);
		overflow: hidden;
		background: #fff;
	}
	.cover {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		padding: 24px;
		background: color-mix(in srgb, var(--pp-ink) 60%, transparent);
		color: #fff;
		text-align: center;
	}
	.cover b {
		font: 800 40px var(--pp-font-display);
	}
</style>
