<script lang="ts">
	/**
	 * The timeline under the canvas: the scene and cut, a frame ruler, and a row per
	 * layer (top layer first) with its exposures. Click a frame to go there; drag an
	 * exposure to move it, or its right edge to hold it longer or shorter.
	 *
	 * The timeline is changed in place; the studio is told (`onedit`) so it can show
	 * the right drawings and count the change as unsaved.
	 */
	import type { LayerInfo } from '@pencilpilot/paint/PaintCanvas.svelte';
	import {
		addCut,
		addScene,
		cutLength,
		deleteFrameAll,
		exposureAt,
		insertExposure,
		insertFrameAll,
		moveExposure,
		removeExposure,
		setLength,
		setTiming,
		startOf,
		trackIn,
		type Timeline,
		type Track
	} from './timeline';
	import type { PlayMode, PlayRange } from './playback';

	let {
		timeline,
		scene = $bindable(0),
		cut = $bindable(0),
		frame = $bindable(0),
		fps = $bindable(24),
		playing = false,
		playMode = $bindable<PlayMode>('loop'),
		playRange = $bindable<PlayRange | null>(null),
		onplay,
		onrange,
		layers,
		active,
		newDrawing,
		onlayer,
		onedit
	}: {
		timeline: Timeline;
		scene?: number;
		cut?: number;
		frame?: number;
		fps?: number;
		playing?: boolean;
		playMode?: PlayMode;
		/** the frames played (none: the whole cut) */
		playRange?: PlayRange | null;
		onplay: () => void;
		/** loop from / to the current frame */
		onrange: (end: 'from' | 'to') => void;
		/** bottom → top, as the canvas lists them */
		layers: LayerInfo[];
		/** the active layer's id */
		active: number;
		/** a new, empty drawing's id (from the canvas) */
		newDrawing: () => number;
		onlayer: (id: number) => void;
		/** the timeline changed (or the frame did): show it */
		onedit: (changed: boolean) => void;
	} = $props();

	/** px per frame */
	const CELL = 22;
	const theScene = $derived(timeline.scenes[scene] ?? timeline.scenes[0]);
	const theCut = $derived(theScene.cuts[cut] ?? theScene.cuts[0]);
	const length = $derived(cutLength(theCut));
	/** room to draw past the end */
	const shown = $derived(Math.max(length + 8, 24));
	const rows = $derived([...layers].reverse().filter((l) => !l.adjust));

	const trackOf = (layer: number): Track => trackIn(theCut, layer);
	const activeTrack = $derived(theCut.tracks[active] ?? []);
	const here = $derived(exposureAt(activeTrack, frame));

	function go(f: number) {
		frame = Math.max(0, f);
		onedit(false);
	}

	/** One frame back or on (the frame-step keys). */
	export function step(by: number) {
		go(frame + by);
	}
	function edited() {
		onedit(true);
	}

	/** a drawing's number on its layer: 1, 2, 3… in order */
	function numbers(t: Track): number[] {
		let n = 0;
		return t.map((e) => (e.drawing ? ++n : 0));
	}

	// ---- buttons ----
	/** a new, empty drawing after this one (or here, past the end), and go to it */
	export function addDrawing() {
		const t = trackOf(active);
		const d = newDrawing();
		if (!d) return;
		if (here) {
			const at = here.index + 1;
			insertExposure(t, at, { drawing: d, length: t[here.index].length });
			frame = startOf(t, at);
		} else {
			const end = t.reduce((n, e) => n + e.length, 0);
			if (frame > end) insertExposure(t, t.length, { drawing: 0, length: frame - end });
			insertExposure(t, t.length, { drawing: d, length: 1 });
		}
		edited();
	}
	function hold(by: number) {
		if (!here) return;
		setLength(activeTrack, here.index, activeTrack[here.index].length + by);
		edited();
	}
	function timing(n: number) {
		if (!here) return;
		setLength(activeTrack, here.index, n);
		edited();
	}
	function timingAll(n: number) {
		setTiming(trackOf(active), n);
		edited();
	}
	function removeHere() {
		if (!here) return;
		removeExposure(activeTrack, here.index);
		edited();
	}
	function insertFrame() {
		insertFrameAll(theCut, frame);
		edited();
	}
	function deleteFrame() {
		deleteFrameAll(theCut, frame);
		frame = Math.min(frame, cutLength(theCut) - 1);
		edited();
	}
	function newCut() {
		cut = addCut(theScene, cut);
		frame = 0;
		edited();
	}
	function newScene() {
		scene = addScene(timeline, scene);
		cut = 0;
		frame = 0;
		edited();
	}

	// ---- dragging an exposure (move) or its edge (hold) ----
	let drag = $state<{
		layer: number;
		index: number;
		mode: 'move' | 'hold';
		x0: number;
		length0: number;
		/** frames moved so far */
		by: number;
	} | null>(null);

	function dragStart(e: PointerEvent, layer: number, index: number, mode: 'move' | 'hold') {
		if (e.button !== 0) return;
		e.stopPropagation();
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		const t = trackOf(layer);
		drag = { layer, index, mode, x0: e.clientX, length0: t[index].length, by: 0 };
		onlayer(layer);
		frame = startOf(t, index);
		onedit(false);
	}
	function dragMove(e: PointerEvent) {
		if (!drag) return;
		const by = Math.round((e.clientX - drag.x0) / CELL);
		if (by === drag.by) return;
		drag.by = by;
		if (drag.mode === 'hold') {
			setLength(trackOf(drag.layer), drag.index, drag.length0 + by);
			edited();
		}
	}
	function dragEnd() {
		if (!drag) return;
		const d = drag;
		drag = null;
		if (d.mode !== 'move' || !d.by) return;
		const t = trackOf(d.layer);
		// drop it where the pointer landed, before or after the exposure there
		const target = startOf(t, d.index) + d.by;
		const at = exposureAt(t, Math.max(0, target));
		let to = at ? at.index : t.length - 1;
		if (to > d.index && at && target < at.start + t[at.index].length / 2) to--;
		if (to < d.index && at && target >= at.start + t[at.index].length / 2) to++;
		to = Math.max(0, Math.min(t.length - 1, to));
		moveExposure(t, d.index, to);
		frame = startOf(t, Math.min(to, t.length - 1));
		edited();
	}

	function cellClick(e: MouseEvent, layer: number) {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		onlayer(layer);
		go(Math.floor((e.clientX - box.left) / CELL));
	}

	const seconds = (f: number) => `${Math.floor(f / fps)}+${String(f % fps).padStart(2, '0')}`;

	// ---- scrubbing: drag along the frame numbers ----
	let scrubbing = false;
	function scrubAt(e: PointerEvent) {
		const ruler = e.currentTarget as HTMLElement;
		const f = Math.floor((e.clientX - ruler.getBoundingClientRect().left) / CELL);
		if (f !== frame) go(Math.max(0, Math.min(shown - 1, f)));
	}
	const inRange = (f: number) => !!playRange && f >= playRange.from && f <= playRange.to;
</script>

<section class="timeline" aria-label="Timeline">
	<div class="bar">
		<label class="pick">
			<span class="sr">Scene</span>
			<select
				aria-label="Scene"
				value={scene}
				onchange={(e) => {
					scene = +e.currentTarget.value;
					cut = 0;
					go(0);
				}}
			>
				{#each timeline.scenes as s, i (s.id)}<option value={i}>{s.name}</option>{/each}
			</select>
		</label>
		<label class="pick">
			<select
				aria-label="Cut"
				value={cut}
				onchange={(e) => {
					cut = +e.currentTarget.value;
					go(0);
				}}
			>
				{#each theScene.cuts as c, i (c.id)}<option value={i}>{c.name}</option>{/each}
			</select>
		</label>
		<button type="button" class="b" title="A new cut after this one" onclick={newCut}>+ Cut</button>
		<button type="button" class="b" title="A new scene after this one" onclick={newScene}>
			+ Scene
		</button>
		<span class="sep"></span>
		<button
			type="button"
			class="b play"
			aria-pressed={playing}
			aria-label={playing ? 'Stop' : 'Play'}
			title={playing ? 'Stop' : 'Play the cut (or the loop range)'}
			onclick={onplay}
		>
			{playing ? '■' : '▶'}
		</button>
		<select aria-label="Frames a second" title="Frames a second" bind:value={fps}>
			{#each [6, 8, 12, 15, 24, 25, 30, 48, 50, 60] as n (n)}<option value={n}>
					{n} fps
				</option>{/each}
		</select>
		<div class="seg" role="radiogroup" aria-label="Play">
			{#each [['once', 'Once'], ['loop', 'Loop'], ['pingpong', 'Ping-pong']] as [id, label] (id)}
				<button
					type="button"
					class="b"
					role="radio"
					aria-checked={playMode === id}
					onclick={() => (playMode = id as PlayMode)}
				>
					{label}
				</button>
			{/each}
		</div>
		<button
			type="button"
			class="b"
			title="Play from this frame"
			aria-pressed={!!playRange && playRange.from === frame}
			onclick={() => onrange('from')}
		>
			In
		</button>
		<button
			type="button"
			class="b"
			title="Play to this frame"
			aria-pressed={!!playRange && playRange.to === frame}
			onclick={() => onrange('to')}
		>
			Out
		</button>
		{#if playRange}
			<button type="button" class="b" title="Play the whole cut" onclick={() => (playRange = null)}>
				Frames {playRange.from + 1}–{playRange.to + 1} ×
			</button>
		{/if}
		<span class="sep"></span>
		<button type="button" class="b" aria-label="Previous frame" onclick={() => go(frame - 1)}>
			‹
		</button>
		<span class="where" aria-live="polite">
			Frame <b data-testid="frame">{frame + 1}</b> of {length}
			<small>{seconds(frame)}</small>
		</span>
		<button type="button" class="b" aria-label="Next frame" onclick={() => go(frame + 1)}>›</button>
		<span class="sep"></span>
		<button
			type="button"
			class="b strong"
			title="A new, empty drawing after this one on the active layer"
			onclick={addDrawing}
		>
			New drawing
		</button>
		<div class="seg" role="group" aria-label="Hold this drawing for">
			{#each [1, 2, 3] as n (n)}
				<button
					type="button"
					class="b"
					aria-pressed={here ? activeTrack[here.index].length === n : false}
					disabled={!here}
					title="Hold this drawing for {n} frame{n > 1 ? 's' : ''} (on {n === 1
						? 'ones'
						: n === 2
							? 'twos'
							: 'threes'})"
					onclick={() => timing(n)}
				>
					{n}s
				</button>
			{/each}
		</div>
		<button
			type="button"
			class="b"
			aria-label="Hold longer"
			title="Hold this drawing one frame longer"
			disabled={!here}
			onclick={() => hold(1)}
		>
			+1
		</button>
		<button
			type="button"
			class="b"
			aria-label="Hold shorter"
			title="Hold this drawing one frame less"
			disabled={!here || activeTrack[here.index].length < 2}
			onclick={() => hold(-1)}
		>
			−1
		</button>
		<select
			class="b"
			aria-label="Timing for the whole layer"
			title="Every drawing on this layer on ones, twos or threes"
			value=""
			onchange={(e) => {
				const n = +e.currentTarget.value;
				e.currentTarget.value = '';
				if (n) timingAll(n);
			}}
		>
			<option value="">Layer timing…</option>
			<option value="1">All on ones</option>
			<option value="2">All on twos</option>
			<option value="3">All on threes</option>
		</select>
		<span class="sep"></span>
		<button
			type="button"
			class="b"
			title="One more frame here on every layer (each holds what it shows)"
			onclick={insertFrame}
		>
			Insert frame
		</button>
		<button
			type="button"
			class="b"
			title="Take this frame out of every layer"
			onclick={deleteFrame}
		>
			Delete frame
		</button>
		<button
			type="button"
			class="b"
			title="Take this drawing out of the timeline (the ones after it move back)"
			disabled={!here}
			onclick={removeHere}
		>
			Remove drawing
		</button>
	</div>

	<div class="grid" style="--cell: {CELL}px; --cols: {shown}">
		<div class="corner"></div>
		<div
			class="ruler"
			role="group"
			aria-label="Frames"
			onpointerdown={(e) => {
				if (e.button !== 0) return;
				scrubbing = true;
				(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
				scrubAt(e);
			}}
			onpointermove={(e) => scrubbing && scrubAt(e)}
			onpointerup={() => (scrubbing = false)}
			onpointercancel={() => (scrubbing = false)}
		>
			{#each Array.from({ length: shown }, (_, i) => i) as f (f)}
				<button
					type="button"
					class="tick"
					class:now={f === frame}
					class:past={f >= length}
					class:ranged={inRange(f)}
					aria-label="Frame {f + 1}"
					aria-current={f === frame ? 'true' : undefined}
					onclick={() => go(f)}
				>
					{f === 0 || (f + 1) % 5 === 0 ? f + 1 : ''}
				</button>
			{/each}
		</div>
		{#each rows as l (l.id)}
			{@const t = theCut.tracks[l.id] ?? []}
			{@const nums = numbers(t)}
			<button
				type="button"
				class="name"
				class:on={l.id === active}
				onclick={() => onlayer(l.id)}
				title={l.name}
			>
				{l.name}
			</button>
			<div
				class="row"
				class:on={l.id === active}
				role="presentation"
				data-layer={l.id}
				onclick={(e) => cellClick(e, l.id)}
			>
				<div class="playhead" style="left: {frame * CELL}px"></div>
				{#each t as e, i (i)}
					{@const start = startOf(t, i)}
					{#if e.drawing}
						<button
							type="button"
							class="cel"
							class:here={l.id === active && here?.index === i}
							class:dragging={drag?.layer === l.id && drag.index === i}
							style="left: {start * CELL}px; width: {e.length * CELL}px; {drag?.layer === l.id &&
							drag.index === i &&
							drag.mode === 'move'
								? `transform: translateX(${drag.by * CELL}px)`
								: ''}"
							aria-label="{l.name}: drawing {nums[i]}, frame {start + 1}{e.length > 1
								? `–${start + e.length}`
								: ''}"
							onpointerdown={(ev) => dragStart(ev, l.id, i, 'move')}
							onpointermove={dragMove}
							onpointerup={dragEnd}
							onpointercancel={() => (drag = null)}
							onclick={(ev) => ev.stopPropagation()}
						>
							<span>{nums[i]}</span>
							<span
								class="edge"
								role="presentation"
								title="Drag to hold it longer or shorter"
								onpointerdown={(ev) => dragStart(ev, l.id, i, 'hold')}
								onpointermove={dragMove}
								onpointerup={dragEnd}
							></span>
						</button>
					{/if}
				{/each}
			</div>
		{/each}
	</div>
</section>

<style>
	.timeline {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px 10px 10px;
		border-radius: 18px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font: 13px var(--pp-font-body);
	}
	.bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.bar > * {
		flex: none;
	}
	.b,
	select {
		height: 32px;
		padding: 0 10px;
		border: 0;
		border-radius: 10px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.b:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.b[aria-pressed='true'],
	.b[aria-checked='true'],
	.b.strong {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.seg {
		display: flex;
		gap: 2px;
	}
	.sep {
		width: 1px;
		height: 22px;
		background: rgba(255, 255, 255, 0.15);
	}
	.where {
		min-width: 120px;
		text-align: center;
	}
	.where small {
		color: var(--pp-on-ink-muted);
		margin-left: 4px;
	}
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
	.grid {
		display: grid;
		grid-template-columns: 110px calc(var(--cell) * var(--cols));
		max-height: 168px;
		overflow: auto;
		scrollbar-width: thin;
	}
	.corner,
	.ruler {
		position: sticky;
		top: 0;
		z-index: 2;
		background: var(--pp-ink);
	}
	.corner {
		left: 0;
		z-index: 3;
	}
	.ruler {
		display: flex;
		height: 20px;
		touch-action: none;
	}
	.tick {
		flex: none;
		width: var(--cell);
		padding: 0;
		border: 0;
		border-left: 1px solid rgba(255, 255, 255, 0.08);
		background: none;
		color: var(--pp-on-ink-muted);
		font: 600 10px var(--pp-font-body);
		cursor: pointer;
	}
	.tick.past {
		opacity: 0.45;
	}
	.tick.ranged {
		background: rgba(255, 214, 80, 0.22);
		color: var(--pp-on-ink);
	}
	.tick.now {
		background: var(--pp-accent-strong);
		color: #fff;
		border-radius: 6px 6px 0 0;
	}
	.name {
		position: sticky;
		left: 0;
		z-index: 1;
		height: 28px;
		margin: 2px 0;
		padding: 0 8px;
		border: 0;
		border-radius: 8px 0 0 8px;
		background: #1f1f24;
		color: var(--pp-on-ink-muted);
		font: 600 12px var(--pp-font-body);
		text-align: left;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		cursor: pointer;
	}
	.name.on {
		color: #fff;
		background: #34343c;
	}
	.row {
		position: relative;
		height: 28px;
		margin: 2px 0;
		background: repeating-linear-gradient(
			90deg,
			#1f1f24 0 calc(var(--cell) - 1px),
			#2a2a30 calc(var(--cell) - 1px) var(--cell)
		);
		cursor: pointer;
	}
	.row.on {
		background: repeating-linear-gradient(
			90deg,
			#28282f 0 calc(var(--cell) - 1px),
			#36363e calc(var(--cell) - 1px) var(--cell)
		);
	}
	.playhead {
		position: absolute;
		top: 0;
		bottom: 0;
		width: var(--cell);
		background: rgba(255, 90, 60, 0.18);
		pointer-events: none;
	}
	.cel {
		position: absolute;
		top: 3px;
		bottom: 3px;
		display: flex;
		align-items: center;
		padding: 0 0 0 6px;
		border: 0;
		border-radius: 6px;
		background: #f4efe3;
		color: var(--pp-ink);
		font: 700 11px var(--pp-font-body);
		cursor: grab;
		touch-action: none;
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
	}
	.cel.here {
		background: var(--pp-yellow);
	}
	.cel.dragging {
		z-index: 2;
		opacity: 0.85;
		cursor: grabbing;
	}
	.edge {
		position: absolute;
		top: 0;
		right: 0;
		bottom: 0;
		width: 7px;
		border-radius: 0 6px 6px 0;
		background: rgba(0, 0, 0, 0.18);
		cursor: ew-resize;
	}
</style>
