<script lang="ts">
	import Tile from '../ui/Tile.svelte';
	import Button from '../ui/Button.svelte';
	import PaintCanvas, {
		BLANK_STATUS,
		type CanvasStatus,
		type LiveEvent
	} from '../paint/PaintCanvas.svelte';
	import RefViewer from '../paint/RefViewer.svelte';
	import MiniToolbar from './MiniToolbar.svelte';
	import { DEFAULT_BRUSH } from '../paint/brushes';
	import { pickRefs } from './refs';
	import type { ClientMsg, LobbyView, ServerMsg, WireStroke } from './protocol';

	let { lobby, you, send }: { lobby: LobbyView; you: string; send: (m: ClientMsg) => void } =
		$props();

	let canvas: ReturnType<typeof PaintCanvas> | undefined = $state();
	let status = $state<CanvasStatus>(BLANK_STATUS);
	let brushId = $state(DEFAULT_BRUSH);
	let color = $state('#16161a');
	let size = $state(1);
	let opacity = $state(1);
	let eraser = $state(false);
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

<div class="free">
	<div class="main">
		<div class="canvas-tile">
			<PaintCanvas
				bind:this={canvas}
				width={lobby.settings.width}
				height={lobby.settings.height}
				{brushId}
				{eraser}
				{size}
				{opacity}
				{color}
				onchange={(s) => (status = s)}
				onlive={live}
				maxHeight="72vh"
			/>
		</div>
		<MiniToolbar
			bind:brushId
			bind:color
			bind:size
			bind:opacity
			bind:eraser
			canUndo={status.canUndo}
			onundo={undo}
			onclear={clearMine}
			clearLabel="Clear my lines"
		/>
	</div>
	<aside class="side">
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
					By <a href={lobby.reference.authorUrl} target="_blank" rel="noreferrer">
						{lobby.reference.author}
					</a> via Pixabay
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
		<Tile tone="lilac">
			<div class="row">
				<Button variant="dark" onclick={save}>Save the picture</Button>
				{#if host}
					<Button variant="outline" onclick={() => send({ t: 'end' })}>Back to the lobby</Button>
				{/if}
			</div>
		</Tile>
	</aside>
</div>

<style>
	.free {
		display: grid;
		grid-template-columns: minmax(0, 1fr) 320px;
		gap: var(--pp-gap);
		align-items: start;
	}
	@media (max-width: 960px) {
		.free {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.main,
	.side {
		display: flex;
		flex-direction: column;
		gap: var(--pp-gap);
		min-width: 0;
	}
	.canvas-tile {
		border-radius: var(--pp-radius-tile);
		overflow: hidden;
		background: #fff;
	}
	h2 {
		font-size: 20px;
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
</style>
