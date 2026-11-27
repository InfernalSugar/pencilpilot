<script lang="ts">
	/**
	 * The new layout's slim strips under the canvas, for the pages without the full
	 * timeline: Draw (the active layer's drawings, one after another, and New), Colour
	 * (the drawings to colour) and Review (a player: the cut, its markers, Play).
	 * They work on the same timeline as the timeline does.
	 */
	import type { LayerInfo } from '@pencilpilot/paint/PaintCanvas.svelte';
	import { cutLength, cutStart, type Timeline } from './timeline';
	import type { OnionSettings } from './onion';
	import type { PlayMode } from './playback';
	import { clock, framesLabel, stripAt, stripDrawings } from './strips';
	import type { MenuItem } from './ContextMenu.svelte';

	let {
		kind,
		timeline,
		scene,
		cut,
		frame = $bindable(0),
		fps,
		layers,
		active,
		onion,
		playing = false,
		playMode = $bindable<PlayMode>('loop'),
		playScope = $bindable<'cut' | 'scene'>('cut'),
		onplay,
		onnew,
		onlayers,
		onopen,
		onmenu,
		onedit
	}: {
		kind: 'draw' | 'colour' | 'review';
		timeline: Timeline;
		scene: number;
		cut: number;
		frame?: number;
		fps: number;
		layers: LayerInfo[];
		active: number;
		onion: OnionSettings;
		playing?: boolean;
		playMode?: PlayMode;
		playScope?: 'cut' | 'scene';
		onplay: () => void;
		/** a new drawing after this one (as the timeline's New drawing) */
		onnew: () => void;
		/** open the Layers panel */
		onlayers: () => void;
		/** open this drawing in another page (Draw) */
		onopen?: (page: string) => void;
		/** a right-click menu at the pointer */
		onmenu: (e: MouseEvent, label: string, items: MenuItem[]) => void;
		/** the frame changed */
		onedit: () => void;
	} = $props();

	const theScene = $derived(timeline.scenes[scene] ?? timeline.scenes[0]);
	const theCut = $derived(theScene.cuts[cut] ?? theScene.cuts[0]);
	const length = $derived(cutLength(theCut, timeline.statics));
	const layer = $derived(layers.find((l) => l.id === active));
	const still = $derived(timeline.statics.includes(active));
	const list = $derived(still ? [] : stripDrawings(theCut.tracks[active] ?? []));
	const here = $derived(stripAt(list, frame));
	const at = $derived(here ? list.indexOf(here) : -1);

	function go(f: number) {
		frame = Math.max(0, f);
		onedit();
	}
	/** the drawing before or after this one */
	function step(by: number) {
		if (!list.length) return;
		const i =
			at >= 0
				? at + by
				: by > 0
					? list.findIndex((d) => d.start > frame)
					: list.findLastIndex((d) => d.start < frame);
		const d = list[Math.max(0, Math.min(list.length - 1, i))];
		if (d) go(d.start);
	}

	// ---- Review: the cut as a player, with the scene's markers in it ----
	const sceneAt = $derived(cutStart(theScene, cut, timeline.statics));
	const marks = $derived(
		(theScene.markers ?? [])
			.map((m) => ({ ...m, f: m.frame - sceneAt }))
			.filter((m) => m.f >= 0 && m.f < length)
	);
	const pct = (f: number) => (length > 1 ? (f / (length - 1)) * 100 : 0);
	function seek(e: PointerEvent) {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const k = Math.max(0, Math.min(1, (e.clientX - box.left) / box.width));
		go(Math.round(k * (length - 1)));
	}
	let seeking = false;
	function markStep(by: number) {
		const next =
			by > 0 ? marks.find((m) => m.f > frame) : [...marks].reverse().find((m) => m.f < frame);
		if (next) go(next.f);
	}
	function playerMenu(e: MouseEvent) {
		onmenu(e, 'Player', [
			{ head: theCut.name },
			{ label: 'Play this cut', checked: playScope === 'cut', run: () => (playScope = 'cut') },
			{
				label: 'Play the whole scene',
				checked: playScope === 'scene',
				run: () => (playScope = 'scene')
			},
			{ sep: true },
			{ label: 'Once', checked: playMode === 'once', run: () => (playMode = 'once') },
			{ label: 'Loop', checked: playMode === 'loop', run: () => (playMode = 'loop') },
			{
				label: 'Ping-pong',
				checked: playMode === 'pingpong',
				run: () => (playMode = 'pingpong')
			},
			{ sep: true },
			{ label: 'Show onion skin', checked: onion.on, run: () => (onion.on = !onion.on) },
			...(onopen ? [{ label: 'Open in Animate', run: () => onopen('animate') }] : [])
		]);
	}
	function drawingMenu(e: MouseEvent, d: (typeof list)[number]) {
		go(d.start);
		onmenu(e, 'Drawing', [
			{ head: `Drawing ${d.n} · ${framesLabel(d)}` },
			{ label: 'Go to it', run: () => go(d.start) },
			...(onopen ? [{ label: 'Open in Draw', run: () => onopen('draw') }] : [])
		]);
	}
</script>

{#snippet icon(d: string, size = 14)}
	<svg
		width={size}
		height={size}
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2.6"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<path {d} />
	</svg>
{/snippet}

{#if kind === 'draw'}
	<section class="strip" aria-label="Frame strip">
		<button
			type="button"
			class="b"
			aria-label="Layer: {layer?.name ?? '—'} (open the layers)"
			title="The layer you draw on (opens the Layers panel)"
			onclick={onlayers}
		>
			{@render icon('M12 3 3 8l9 5 9-5Z M3 13l9 5 9-5', 16)}
			{layer?.name ?? '—'}
		</button>
		<button
			type="button"
			class="ib onion"
			aria-pressed={onion.on}
			aria-label="Onion skin"
			title="Onion skin (O)"
			onclick={() => (onion.on = !onion.on)}
		>
			<svg
				width="16"
				height="14"
				viewBox="0 0 28 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2.4"
				aria-hidden="true"
			>
				<circle cx="10" cy="12" r="7" /><circle cx="18" cy="12" r="7" stroke-opacity="0.45" />
			</svg>
		</button>
		<span class="grow"></span>
		<button type="button" class="ib" aria-label="Previous drawing" onclick={() => step(-1)}>
			{@render icon('M15 5l-7 7 7 7')}
		</button>
		<div class="chips" role="group" aria-label="Drawings in this cut">
			{#each list as d (d.index)}
				<button
					type="button"
					class="chip"
					aria-label="Drawing {d.n}, {framesLabel(d)}"
					aria-current={here === d ? 'true' : undefined}
					onclick={() => go(d.start)}
					oncontextmenu={(e) => drawingMenu(e, d)}
				>
					{d.n}
				</button>
			{:else}
				<span class="muted">
					{still ? 'A static layer: one drawing all cut long' : 'No drawings yet'}
				</span>
			{/each}
		</div>
		<button type="button" class="ib" aria-label="Next drawing" onclick={() => step(1)}>
			{@render icon('M9 5l7 7-7 7')}
		</button>
		<span class="grow"></span>
		<span class="where" aria-live="polite">
			{#if here}Drawing {here.n} <span class="muted">· {framesLabel(here)}</span>{:else}Frame
				{frame + 1}{/if}
		</span>
		<button
			type="button"
			class="b new"
			aria-label="New drawing"
			title="A new, empty drawing after this one (N)"
			disabled={still}
			onclick={onnew}
		>
			{@render icon('M12 5v14M5 12h14', 16)} New
		</button>
	</section>
{:else if kind === 'colour'}
	<section class="strip col" aria-label="Drawings to colour">
		<div class="row">
			<b>{theCut.name} · {layer?.name ?? '—'}</b>
			<span class="muted">{list.length} drawing{list.length === 1 ? '' : 's'}</span>
			<span class="grow"></span>
			<span class="muted tip">Right-click a drawing for more</span>
		</div>
		<div class="chips wide" role="group" aria-label="Drawings">
			{#each list as d (d.index)}
				<button
					type="button"
					class="card"
					aria-label="Drawing {d.n}, {framesLabel(d)}"
					aria-current={here === d ? 'true' : undefined}
					onclick={() => go(d.start)}
					oncontextmenu={(e) => drawingMenu(e, d)}
				>
					<b>Drawing {d.n}</b>
					<span class="muted">{framesLabel(d)}</span>
				</button>
			{:else}
				<span class="muted">
					{still ? 'A static layer: one drawing all cut long' : 'No drawings on this layer yet'}
				</span>
			{/each}
		</div>
	</section>
{:else}
	<section class="strip col" aria-label="Review player" oncontextmenu={playerMenu}>
		<div
			class="track"
			role="slider"
			tabindex="0"
			aria-label="Where in the cut"
			aria-valuemin={1}
			aria-valuemax={length}
			aria-valuenow={frame + 1}
			onpointerdown={(e) => {
				(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
				seeking = true;
				seek(e);
			}}
			onpointermove={(e) => seeking && seek(e)}
			onpointerup={() => (seeking = false)}
			onkeydown={(e) => {
				const by = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
				if (!by) return;
				e.preventDefault();
				e.stopPropagation();
				go(Math.min(length - 1, frame + by));
			}}
		>
			<span class="rail"></span>
			<span class="done" style:width="{pct(frame)}%"></span>
			{#each marks as m (m.frame)}
				<span class="pin" style:left="{pct(m.f)}%" title={m.label || `Marker, frame ${m.f + 1}`}
				></span>
			{/each}
			<span class="knob" style:left="{pct(frame)}%"></span>
		</div>
		<div class="row">
			<button
				type="button"
				class="ib big"
				aria-label="Previous marker"
				disabled={!marks.some((m) => m.f < frame)}
				onclick={() => markStep(-1)}
			>
				{@render icon('M15 5l-7 7 7 7')}
			</button>
			<button
				type="button"
				class="ib big play"
				aria-pressed={playing}
				aria-label={playing ? 'Stop' : 'Play'}
				onclick={onplay}
			>
				{#if playing}
					<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true">
						<rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor" />
					</svg>
				{:else}
					<svg width="15" height="15" viewBox="0 0 24 24" aria-hidden="true">
						<path d="M7 4v16l13-8Z" fill="currentColor" />
					</svg>
				{/if}
			</button>
			<button
				type="button"
				class="ib big"
				aria-label="Next marker"
				disabled={!marks.some((m) => m.f > frame)}
				onclick={() => markStep(1)}
			>
				{@render icon('M9 5l7 7-7 7')}
			</button>
			<span class="where">
				{clock(frame, fps)}
				<span class="muted">/ {clock(length, fps)} · frame {frame + 1}</span>
			</span>
			<span class="grow"></span>
			<span class="muted tip">Right-click the player for loop and what plays</span>
		</div>
	</section>
{/if}

<style>
	.strip {
		position: absolute;
		left: 12px;
		right: 12px;
		bottom: 12px;
		z-index: 18;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		padding: 8px 12px;
		border-radius: 22px;
		background: #16161a;
		color: #f6f4ef;
		font: 600 13px var(--pp-font-body);
		box-shadow: 0 10px 30px rgba(22, 22, 26, 0.25);
	}
	.strip.col {
		flex-direction: column;
		align-items: stretch;
		padding: 10px 14px;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}
	.grow {
		flex: 1 1 0;
	}
	.muted {
		color: #a9a8ae;
		font-weight: 500;
	}
	.tip {
		font-size: 11px;
	}
	.b,
	.ib {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		height: 36px;
		border: 0;
		border-radius: 10px;
		background: #2a2a30;
		color: #f6f4ef;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.b {
		padding: 0 12px;
	}
	.ib {
		width: 36px;
		padding: 0;
	}
	.ib.onion {
		width: 40px;
	}
	.ib[aria-pressed='true'] {
		background: var(--pp-yellow, #ffd84d);
		color: #16161a;
	}
	.ib.play {
		width: 52px;
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.ib.play[aria-pressed='true'] {
		background: #f6f4ef;
		color: #16161a;
	}
	.b.new {
		gap: 6px;
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.b:disabled,
	.ib:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.chips {
		display: flex;
		gap: 4px;
		min-width: 0;
		max-width: 50%;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.chips.wide {
		max-width: none;
		gap: 8px;
	}
	.chip {
		flex: none;
		width: 44px;
		height: 36px;
		border: 0;
		border-radius: 8px;
		background: #f6f4ef;
		color: #16161a;
		font: 700 11px var(--pp-font-body);
		cursor: pointer;
	}
	.chip[aria-current='true'] {
		box-shadow: inset 0 0 0 3px var(--pp-accent-strong, #e8492f);
	}
	.card {
		flex: none;
		display: flex;
		flex-direction: column;
		gap: 4px;
		width: 120px;
		padding: 8px;
		border: 2px solid transparent;
		border-radius: 12px;
		background: #24242a;
		color: #f6f4ef;
		font: 600 12px var(--pp-font-body);
		text-align: left;
		cursor: pointer;
	}
	.card[aria-current='true'] {
		border-color: var(--pp-accent-strong, #e8492f);
	}
	.where {
		padding: 0 8px;
		font-variant-numeric: tabular-nums;
	}
	.track {
		position: relative;
		height: 28px;
		cursor: pointer;
		touch-action: none;
		outline: none;
	}
	.track:focus-visible .rail {
		outline: 2px solid #f6f4ef;
	}
	.rail,
	.done {
		position: absolute;
		left: 0;
		top: 11px;
		height: 6px;
		border-radius: 3px;
	}
	.rail {
		right: 0;
		background: #34343c;
	}
	.done {
		background: var(--pp-accent-strong, #e8492f);
	}
	.pin {
		position: absolute;
		top: 2px;
		width: 14px;
		height: 14px;
		margin-left: -7px;
		border: 2px solid #16161a;
		border-radius: 999px 999px 999px 2px;
		background: var(--pp-yellow, #ffd84d);
	}
	.knob {
		position: absolute;
		top: 6px;
		width: 16px;
		height: 16px;
		margin-left: -8px;
		border-radius: 999px;
		background: #fff;
		box-shadow: 0 0 0 3px var(--pp-accent-strong, #e8492f);
		pointer-events: none;
	}
</style>
