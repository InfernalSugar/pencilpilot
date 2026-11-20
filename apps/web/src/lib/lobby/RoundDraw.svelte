<script lang="ts">
	import { onMount } from 'svelte';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus
	} from '@pencilpilot/paint/PaintCanvas.svelte';
	import RefViewer from '@pencilpilot/paint/RefViewer.svelte';
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail from '@pencilpilot/paint/ToolRail.svelte';
	import Swatches from '@pencilpilot/paint/Swatches.svelte';
	import BrushPicker from '@pencilpilot/paint/BrushPicker.svelte';
	import ColorPanel from '@pencilpilot/paint/ColorPanel.svelte';
	import LayersPanel from '@pencilpilot/paint/LayersPanel.svelte';
	import {
		DEFAULT_BRUSH,
		brushLabel,
		loadManifest,
		type BrushManifest
	} from '@pencilpilot/paint/brushes';
	import { gradeImages, toGray, type GradeCategory } from '../grader';
	import { referencePixels } from '../references';
	import { clockOf } from '../timed';
	import { asReference } from './refs';
	import RefCredit from '../RefCredit.svelte';
	import { gradeCategory } from '../grade-category';
	import type { ClientMsg, LobbyView } from './protocol';

	let {
		lobby,
		you,
		send,
		left,
		reconnecting = false
	}: {
		lobby: LobbyView;
		you: string;
		send: (m: ClientMsg) => void;
		/** ms left until a server time */
		left: (t: number) => number;
		reconnecting?: boolean;
	} = $props();

	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let brushId = $state(DEFAULT_BRUSH);
	let color = $state('#16161a');
	let size = $state(1);
	let opacity = $state(1);
	let eraser = $state(false);
	let tool = $state<'brush' | 'picker' | 'fill'>('brush');
	let tab = $state('layers');
	let dockOpen = $state(false);
	let manifest = $state<BrushManifest | null>(null);
	const brushName = $derived(brushLabel(manifest, brushId));
	const activeLayerName = $derived(status.layers.find((l) => l.id === status.active)?.name ?? '');
	let handedIn = $state(false);
	let grading = $state(false);
	let remaining = $state(0);

	const done = $derived(lobby.submitted.includes(you) || handedIn);
	const waitingFor = $derived(
		lobby.players.filter((p) => p.connected && !lobby.submitted.includes(p.id))
	);

	onMount(() => {
		loadManifest().then((m) => (manifest = m));
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
			const cat = gradeCategory(ref.category);
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

<Studio
	backHref="/lobby"
	tabs={[
		{ id: 'layers', label: 'Layers', meta: activeLayerName },
		{ id: 'brushes', label: 'Brushes', meta: brushName },
		{ id: 'colour', label: 'Colour' }
	]}
	bind:tab
	bind:dockOpen
>
	{#snippet title()}
		<span class="pp-eyebrow">
			Lobby {lobby.code} · Competition · Round {lobby.round} of {lobby.settings
				.rounds}{#if reconnecting}<span class="conn" role="status"> · Reconnecting…</span>{/if}
		</span>
		<h1>{lobby.reference?.title ?? 'Draw!'}</h1>
	{/snippet}
	{#snippet actions()}
		<span
			class="timer"
			class:urgent={remaining <= 10_000}
			role="timer"
			aria-label="Time left {clockOf(remaining / 1000)}"
		>
			{lobby.phase === 'collecting' ? 'Time’s up!' : clockOf(remaining / 1000)}
		</span>
		<button
			type="button"
			class="act"
			aria-label="Undo"
			title="Undo"
			disabled={!status.canUndo || done}
			onclick={() => canvas?.undo()}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" />
			</svg>
		</button>
		<button
			type="button"
			class="act"
			aria-label="Redo"
			title="Redo"
			disabled={!status.canRedo || done}
			onclick={() => canvas?.redo()}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="m15 14 5-5-5-5" /><path d="M20 9H9a5 5 0 0 0 0 10h3" />
			</svg>
		</button>
		{#if !done}
			<Button onclick={submitNow} disabled={status.blank}>I’m done →</Button>
		{/if}
	{/snippet}
	{#snippet rail()}
		<ToolRail
			tools={['brush', 'eraser', 'picker', 'fill']}
			bind:tool
			bind:eraser
			bind:size
			bind:opacity
		/>
	{/snippet}
	{#snippet options()}
		<span class="brush-name">{brushName}</span>
		<Swatches
			bind:color
			onmore={() => {
				tab = 'colour';
				dockOpen = true;
			}}
			moreOpen={tab === 'colour' && dockOpen}
		/>
		<button type="button" class="opt" disabled={done} onclick={() => canvas?.clear()}>
			Clear layer
		</button>
	{/snippet}
	{#snippet stage()}
		{#key lobby.round}
			<PaintCanvas
				bind:this={canvas}
				fill
				width={lobby.settings.width}
				height={lobby.settings.height}
				{brushId}
				{eraser}
				{size}
				{opacity}
				{color}
				{tool}
				onpick={(c) => {
					color = c;
					eraser = false;
					tool = 'brush';
				}}
				locked={done || lobby.phase === 'collecting'}
				onchange={(s) => (status = s)}
			/>
		{/key}
		{#if done}
			<div class="cover" role="status">
				<b>{grading ? 'Grading…' : 'Handed in!'}</b>
				<span>Waiting for the others. Nobody sees your drawing until the reveal.</span>
			</div>
		{/if}
	{/snippet}
	{#snippet dockTop()}
		{#if lobby.reference}
			<Tile as="section" label="Reference">
				<h2>Reference</h2>
				{#key lobby.reference.id}
					<RefViewer src={lobby.reference.imageUrl} alt="Reference: {lobby.reference.title}" />
				{/key}
				<p class="credit">
					<RefCredit reference={lobby.reference} license={false} />
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
		</Tile>
	{/snippet}
	{#snippet panel(t: string)}
		{#if t === 'layers'}
			<section aria-label="Layers">
				<LayersPanel {status} controls={canvas} />
			</section>
		{:else if t === 'brushes'}
			<section aria-label="Brushes">
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
				{:else}
					<p class="credit">Loading brushes…</p>
				{/if}
			</section>
		{:else}
			<ColorPanel bind:color onclose={() => (tab = 'layers')} />
		{/if}
	{/snippet}
</Studio>

<style>
	.timer {
		min-width: 92px;
		padding: 7px 14px;
		border-radius: 12px;
		background: var(--pp-yellow);
		color: var(--pp-ink);
		font: 700 18px var(--pp-font-display);
		text-align: center;
		font-variant-numeric: tabular-nums;
	}
	.timer.urgent {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	h2 {
		font-size: 18px;
	}
	.credit {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.conn {
		color: var(--pp-accent-ink);
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
	.brush-name {
		font: 700 15px var(--pp-font-display);
		white-space: nowrap;
	}
	.opt {
		height: 38px;
		padding: 0 12px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.opt:disabled {
		opacity: 0.35;
	}
	.act {
		flex: none;
		width: 40px;
		height: 40px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-ink);
		color: #fff;
		cursor: pointer;
	}
	.act:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.act svg {
		width: 20px;
		height: 20px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.3;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
</style>
