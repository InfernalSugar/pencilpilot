<script lang="ts">
	import Tile from '../ui/Tile.svelte';
	import Button from '../ui/Button.svelte';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus,
		type LiveEvent
	} from '../paint/PaintCanvas.svelte';
	import RefViewer from '../paint/RefViewer.svelte';
	import Studio from '../paint/Studio.svelte';
	import ToolRail from '../paint/ToolRail.svelte';
	import Swatches from '../paint/Swatches.svelte';
	import BrushPicker from '../paint/BrushPicker.svelte';
	import ColorPanel from '../paint/ColorPanel.svelte';
	import { DEFAULT_BRUSH, brushLabel, loadManifest, type BrushManifest } from '../paint/brushes';
	import { onMount } from 'svelte';
	import { pickRefs } from './refs';
	import RefCredit from '../RefCredit.svelte';
	import type { ClientMsg, LobbyView, ServerMsg, WireStroke } from './protocol';

	let {
		lobby,
		you,
		send,
		reconnecting = false
	}: {
		lobby: LobbyView;
		you: string;
		send: (m: ClientMsg) => void;
		reconnecting?: boolean;
	} = $props();

	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let brushId = $state(DEFAULT_BRUSH);
	let color = $state('#16161a');
	let size = $state(1);
	let opacity = $state(1);
	let eraser = $state(false);
	/** only tools whose strokes are shared live: the others would show differently for everyone */
	let tool = $state<'brush' | 'picker'>('brush');
	let tab = $state('brushes');
	let dockOpen = $state(false);
	let manifest = $state<BrushManifest | null>(null);
	onMount(() => {
		loadManifest().then((m) => (manifest = m));
	});
	const brushName = $derived(brushLabel(manifest, brushId));
	let picking = $state(false);

	const host = $derived(lobby.players.some((p) => p.id === you && p.host));
	const me = $derived(lobby.players.find((p) => p.id === you));
	const nameOf = (id: string) => lobby.players.find((p) => p.id === id)?.name ?? 'Player';

	// Everyone else's strokes, kept here so a layer can be redrawn after their undo.
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- stroke bookkeeping, never rendered
	const others = new Map<string, WireStroke[]>();
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- stroke bookkeeping, never rendered
	const drawing = new Map<string, WireStroke>();
	let pending: ServerMsg[] = [];
	let ready = $derived(status.layers.length > 0);

	$effect(() => {
		if (!ready || !canvas) return;
		canvas.nameOwnLayer(me?.name ?? 'You');
		for (const m of pending) apply(m);
		pending = [];
	});

	/** Messages from the server about the shared canvas. */
	export function onMessage(m: ServerMsg) {
		if (!ready || !canvas) {
			pending.push(m);
			return;
		}
		apply(m);
	}

	function apply(m: ServerMsg) {
		if (!canvas) return;
		switch (m.t) {
			case 'sync':
				others.clear();
				for (const [id, list] of Object.entries(m.strokes)) {
					if (id === you) continue;
					others.set(id, list);
					void canvas.remoteRedraw(id, list, nameOf(id));
				}
				return;
			case 'sb':
				drawing.set(`${m.from}:${m.k}`, { style: m.style, pts: [] });
				return canvas.remoteBegin(m.from, m.k, m.style, nameOf(m.from));
			case 'sp':
				drawing.get(`${m.from}:${m.k}`)?.pts.push(...m.pts);
				return canvas.remotePoints(m.from, m.k, m.pts);
			case 'se': {
				const s = drawing.get(`${m.from}:${m.k}`);
				drawing.delete(`${m.from}:${m.k}`);
				if (s) others.set(m.from, [...(others.get(m.from) ?? []), s]);
				return canvas.remoteEnd(m.from, m.k);
			}
			case 'undo':
			case 'clear': {
				const list = m.t === 'undo' ? (others.get(m.from) ?? []).slice(0, -1) : [];
				others.set(m.from, list);
				return void canvas.remoteRedraw(m.from, list, nameOf(m.from));
			}
		}
	}

	function live(e: LiveEvent) {
		if (e.type === 'begin') send({ t: 'sb', k: e.k, style: e.style });
		else if (e.type === 'points') send({ t: 'sp', k: e.k, pts: e.pts });
		else if (e.type === 'end') send({ t: 'se', k: e.k });
		else {
			send({ t: 'se', k: e.k });
			send({ t: 'undo' });
		}
	}

	function undo() {
		if (!status.canUndo) return;
		canvas?.undo();
		send({ t: 'undo' });
	}
	function clearMine() {
		canvas?.clear();
		send({ t: 'clear' });
	}

	async function randomPicture() {
		picking = true;
		try {
			const [r] = await pickRefs(lobby.settings.category, lobby.settings.tier, 1);
			if (r) send({ t: 'reference', ref: r });
		} finally {
			picking = false;
		}
	}

	function save() {
		const d = canvas?.exportDrawing();
		if (!d) return;
		const a = document.createElement('a');
		a.href = d.png;
		a.download = `pencilpilot-${lobby.code}.png`;
		a.click();
	}
</script>

<svelte:window
	onkeydown={(e) => {
		if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !e.shiftKey) {
			e.preventDefault();
			undo();
		}
	}}
/>

<Studio
	backHref="/lobby"
	tabs={[
		{ id: 'brushes', label: 'Brushes', meta: brushName },
		{ id: 'colour', label: 'Colour' }
	]}
	bind:tab
	bind:dockOpen
>
	{#snippet title()}
		<span class="pp-eyebrow">
			Lobby {lobby.code} · Free draw{#if reconnecting}<span class="conn" role="status">
					· Reconnecting…
				</span>{/if}
		</span>
		<h1>Drawing together</h1>
	{/snippet}
	{#snippet actions()}
		<button
			type="button"
			class="act"
			aria-label="Undo"
			title="Undo (your own lines)"
			disabled={!status.canUndo}
			onclick={undo}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" />
			</svg>
		</button>
		<Button variant="dark" onclick={save}>Save the picture</Button>
		{#if host}
			<Button variant="outline" onclick={() => send({ t: 'end' })}>Back to the lobby</Button>
		{/if}
	{/snippet}
	{#snippet rail()}
		<ToolRail tools={['brush', 'eraser', 'picker']} bind:tool bind:eraser bind:size bind:opacity />
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
		<button type="button" class="opt" onclick={clearMine}>Clear my lines</button>
	{/snippet}
	{#snippet stage()}
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
			onchange={(s) => (status = s)}
			onlive={live}
			ongesture={(g) => g === 'undo' && undo()}
		/>
	{/snippet}
	{#snippet dockTop()}
		<Tile as="section" label="Who’s drawing">
			<h2>Drawing together</h2>
			<ul class="players">
				{#each lobby.players as p (p.id)}
					<li class:away={!p.connected}>
						<span class="dot" aria-hidden="true"></span>{p.name}{p.id === you ? ' (you)' : ''}
					</li>
				{/each}
			</ul>
			<p class="help">Each person draws on their own layer. Undo only takes back your own lines.</p>
		</Tile>
		<Tile as="section" label="Reference">
			<h2>Reference</h2>
			{#if lobby.reference}
				{#key lobby.reference.id}
					<RefViewer src={lobby.reference.imageUrl} alt="Reference: {lobby.reference.title}" />
				{/key}
				<p class="credit">
					<RefCredit reference={lobby.reference} license={false} />
				</p>
			{:else}
				<p class="help">No picture: draw whatever you like.</p>
			{/if}
			<div class="row">
				<Button variant="outline" onclick={randomPicture} disabled={picking}>
					{picking ? 'Finding…' : lobby.reference ? 'Another picture' : 'Show a picture'}
				</Button>
				{#if lobby.reference}
					<Button variant="outline" onclick={() => send({ t: 'reference', ref: null })}>
						Remove
					</Button>
				{/if}
			</div>
		</Tile>
	{/snippet}
	{#snippet panel(t: string)}
		{#if t === 'brushes'}
			<section aria-label="Brushes">
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
				{:else}
					<p class="help">Loading brushes…</p>
				{/if}
			</section>
		{:else}
			<ColorPanel bind:color onclose={() => (tab = 'brushes')} />
		{/if}
	{/snippet}
</Studio>

<style>
	h2 {
		font-size: 18px;
	}
	.players {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-weight: 600;
	}
	.players li {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.dot {
		width: 10px;
		height: 10px;
		border-radius: 999px;
		background: #1f9d74;
	}
	.away .dot {
		background: var(--pp-track);
	}
	.help,
	.credit {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.conn {
		color: var(--pp-accent-ink);
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
