<script lang="ts">
	/**
	 * The current tool's options, in a row under the studio's options bar: guides, the
	 * Adjust panel, gradient, liquify, transform, selection, fill, smudge, clone, pen path,
	 * measure and lasso fill. The settings live in a `ToolOptions` (tool-options.svelte.ts).
	 */
	import ToneEditor from './ToneEditor.svelte';
	import FilterEditor from './FilterEditor.svelte';
	import TextOptions from './TextOptions.svelte';
	import type PaintCanvas from './PaintCanvas.svelte';
	import type { CanvasStatus } from './PaintCanvas.svelte';
	import {
		drawingGuide,
		arcsPath,
		COMPOSITIONS,
		compositionFlips,
		compositionOn,
		compositionPlaced,
		compositionShapes,
		NO_COMPOSITION,
		SYMMETRY_MODES,
		symmetryOn,
		type CompositionPreset
	} from './guides';
	import type { CompositionDef } from './composition-elements';
	import CompositionCreator from './CompositionCreator.svelte';
	import {
		customCompositions,
		importCompositionFiles,
		onCustomCompositions
	} from './custom-compositions';
	import { MAP_PRESETS } from './tone';
	import type { ToolOptions } from './tool-options.svelte';

	let {
		opts,
		tool = $bindable('brush'),
		status,
		canvas,
		measureUnit = 0,
		eraser = false,
		askToDelete = true
	}: {
		opts: ToolOptions;
		tool?: string;
		/** the brush is the eraser */
		eraser?: boolean;
		status: CanvasStatus;
		canvas: ReturnType<typeof PaintCanvas> | undefined;
		/** the unit marked on the reference, in canvas px (0: measure in pixels) */
		measureUnit?: number;
		/** ask before deleting one of "My compositions" */
		askToDelete?: boolean;
	} = $props();

	/** the composition overlays' names, as the picker shows them */
	const COMPOSITION_NAMES: Record<CompositionPreset, string> = {
		off: 'Off',
		thirds: 'Thirds',
		phi: 'Golden grid',
		spiral: 'Golden spiral',
		triangles: 'Golden triangles',
		diagonals: 'Diagonals',
		cross: 'Centre lines',
		armature: 'Armature',
		safe: 'Safe areas'
	};
	const composition = $derived(opts.guides.composition ?? NO_COMPOSITION);

	// ---- My compositions: guides of your own ----
	let mine = $state<CompositionDef[]>(customCompositions());
	$effect(() => onCustomCompositions(() => (mine = [...customCompositions()])));
	/** the guide in use is one of your own, but not (or no longer) in My compositions */
	const projectOwn = $derived.by(() => {
		const c = composition.custom;
		return composition.preset === 'custom' && c && !mine.some((d) => d.id === c.id) ? c : null;
	});
	const usingOwn = (d: CompositionDef) =>
		composition.preset === 'custom' && composition.custom?.id === d.id;
	/** the creator, open on a guide (or on a new one: null) */
	let creator = $state<{ start: CompositionDef | null; n: number } | null>(null);
	let opened = 0;
	const create = (start: CompositionDef | null) => (creator = { start, n: ++opened });
	let compNote = $state('');
	let compFiles = $state<HTMLInputElement>();

	/** use a guide of your own (its copy goes into the project's guides) */
	const useOwn = (d: CompositionDef) =>
		opts.setComposition({
			preset: 'custom',
			custom: $state.snapshot(d) as CompositionDef,
			flipX: false,
			flipY: false
		});

	async function importComps(list: File[], thenEdit: boolean): Promise<string> {
		if (!list.length) return '';
		const r = await importCompositionFiles(list);
		compNote = r.note;
		if (r.saved[0]) {
			useOwn(r.saved[0]);
			if (thenEdit) create(r.saved[0]);
		}
		return r.note;
	}

	const onLines = $derived(!!status.layers.find((l) => l.id === status.active)?.lines);
	/** how many line strokes are selected (the status bumps on every change) */
	const picked = $derived.by(() => {
		void status.version;
		return canvas?.linesSelected() ?? 0;
	});
	/** the active layer's editable strokes */
	const strokes = $derived.by(() => {
		void status.version;
		return canvas?.layerStrokes(status.active) ?? 0;
	});
	/** Move shows the layer's editable strokes (no layer box yet) */
	const strokesShown = $derived.by(() => {
		void status.version;
		return !!canvas?.moveShowsStrokes();
	});
	/** a stroke picked with another tool (Move, a held brush) is being edited */
	const pickedElsewhere = $derived.by(() => {
		void status.version;
		return !!canvas?.strokePicked();
	});
</script>

<!-- (picked with Move, its bar takes Move's place; with the brush held still, no bar
	comes: the canvas mustn't shift under the pen) -->
{#if tool === 'lines' || (pickedElsewhere && tool === 'transform')}
	<div class="fill-opts" role="group" aria-label="Select line">
		{#if !onLines && !strokes}
			<span class="msg">
				No editable strokes on this layer. Strokes you draw from now on can be picked here.
			</span>
		{:else}
			{#if picked}
				<button type="button" class="tool" onclick={() => canvas?.deleteLines()}>Delete</button>
				<button type="button" class="tool" onclick={() => canvas?.deselectLines()}>
					Deselect
				</button>
			{/if}
			<span
				class="msg"
				data-testid="lines-picked"
				title={picked
					? 'Drag to move (Shift: straight across, down or at 45°); the corners scale (Shift: in proportion), the round handle turns (Shift: in 15° steps), the dots reshape. Arrow keys nudge, Delete removes. The colour, size, opacity and brush you pick change the selected lines.'
					: 'Shift+tap adds or takes away a line; drag on empty canvas for a box.'}
			>
				{picked
					? `${picked} line${picked === 1 ? '' : 's'} selected · new colour, size or brush changes ${picked === 1 ? 'it' : 'them'}`
					: 'Tap a line to select it, or drag a box round some'}
			</span>
		{/if}
	</div>
{/if}
{#if tool === 'brush' && eraser && (onLines || strokes)}
	<div class="fill-opts" role="group" aria-label="Line eraser">
		<span class="opt-label">Erase</span>
		<div class="seg" role="radiogroup" aria-label="Line eraser">
			<button
				type="button"
				class="tool"
				role="radio"
				aria-checked={opts.lineEraser === 'normal'}
				title="Erase where it passes: lines it cuts through stay editable, in pieces"
				onclick={() => (opts.lineEraser = 'normal')}
			>
				Normal
			</button>
			<button
				type="button"
				class="tool"
				role="radio"
				aria-checked={opts.lineEraser === 'whole'}
				title="Remove every line the eraser touches"
				onclick={() => (opts.lineEraser = 'whole')}
			>
				Whole lines
			</button>
		</div>
	</div>
{/if}
{#if tool === 'fill' && onLines}
	<div class="fill-opts" role="note">
		<span class="msg">
			On a line layer, fills go on the layer under it (a new “Colour” layer if needed), inside its
			lines.
		</span>
	</div>
{/if}

{#if opts.guidesOpen}
	<div class="fill-opts guides-opts" role="group" aria-label="Guides">
		<span class="opt-label">Grid</span>
		<div class="seg" role="radiogroup" aria-label="Grid">
			{#each [['off', 'Off'], ['square', 'Square'], ['isometric', 'Isometric']] as [id, label] (id)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={id === 'off'
						? !opts.guides.grid.on
						: opts.guides.grid.on && opts.guides.grid.kind === id}
					title={id === 'off' ? undefined : 'Shift: points snap to the grid'}
					onclick={() =>
						opts.setGuides({
							...opts.guides,
							grid:
								id === 'off'
									? { ...opts.guides.grid, on: false }
									: { ...opts.guides.grid, on: true, kind: id as 'square' | 'isometric' }
						})}
				>
					{label}
				</button>
			{/each}
		</div>
		{#if opts.guides.grid.on}
			<label class="slider">
				<span>Grid size <b>{opts.guides.grid.size}</b></span>
				<input
					type="range"
					min="10"
					max="300"
					step="2"
					value={opts.guides.grid.size}
					oninput={(e) =>
						opts.setGuides({
							...opts.guides,
							grid: { ...opts.guides.grid, size: +e.currentTarget.value }
						})}
				/>
			</label>
		{/if}
		<span class="opt-label">Perspective</span>
		<div class="seg" role="radiogroup" aria-label="Perspective">
			{#each [0, 1, 2, 3] as n (n)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={opts.guides.perspective.points === n}
					onclick={() => opts.setPerspective(n as 0 | 1 | 2 | 3)}
				>
					{n ? `${n}-point` : 'Off'}
				</button>
			{/each}
		</div>
		{#if opts.guides.perspective.points || opts.guides.grid.on}
			{@const layer = status.layers.find((l) => l.id === status.active)}
			<button
				type="button"
				class="tool"
				aria-pressed={!!layer?.assist}
				disabled={!layer}
				title="Guide snap on this layer: strokes snap to the grid, isometric or perspective directions (each layer has its own, in Layers)"
				onclick={() => layer && canvas?.setLayerAssist(layer.id, !layer.assist)}
			>
				Guide snap
			</button>
		{/if}
		<button
			type="button"
			class="tool"
			aria-pressed={opts.inkSnap}
			title="Snap to ink: a stroke starts and ends on a line close by (a line's end first), so lines meet with no gap. Dragged line points too."
			onclick={() => (opts.inkSnap = !opts.inkSnap)}
		>
			Snap to ink
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.guides.ruler.on}
			title="Ruler: strokes started next to it run straight along it"
			onclick={() =>
				opts.setGuides({
					...opts.guides,
					ruler: { ...opts.guides.ruler, on: !opts.guides.ruler.on }
				})}
		>
			Ruler
		</button>
		<span class="opt-label">Symmetry</span>
		<div class="seg" role="radiogroup" aria-label="Symmetry">
			{#each SYMMETRY_MODES as m (m)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={opts.guides.symmetry.mode === m}
					onclick={() =>
						opts.setGuides({ ...opts.guides, symmetry: { ...opts.guides.symmetry, mode: m } })}
				>
					{m === 'off' ? 'Off' : m[0].toUpperCase() + m.slice(1)}
				</button>
			{/each}
		</div>
		{#if opts.guides.symmetry.mode === 'radial'}
			<label class="slider">
				<span>Copies <b>{opts.guides.symmetry.n}</b></span>
				<input
					type="range"
					min="2"
					max="24"
					step="1"
					value={opts.guides.symmetry.n}
					oninput={(e) =>
						opts.setGuides({
							...opts.guides,
							symmetry: { ...opts.guides.symmetry, n: +e.currentTarget.value }
						})}
				/>
			</label>
		{/if}
		{#if opts.guides.perspective.points || opts.guides.ruler.on || symmetryOn(opts.guides)}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'guides'}
				title="Drag vanishing points and the ruler"
				onclick={() => (tool = tool === 'guides' ? 'brush' : 'guides')}
			>
				Move guides
			</button>
		{/if}
		{#if opts.canCentre && drawingGuide(opts.guides)}
			<button
				type="button"
				class="tool"
				title="Put the mirror line, vanishing points and ruler back in the middle of the canvas"
				onclick={() => opts.centreGuides()}
			>
				Re-centre
			</button>
		{/if}
	</div>
{/if}
{#if opts.compositionOpen}
	<div class="fill-opts guides-opts comp-opts" role="group" aria-label="Composition guides">
		<div class="seg" role="radiogroup" aria-label="Composition">
			{#each COMPOSITIONS as p (p)}
				{@const shapes = compositionShapes({ preset: p, flipX: false, flipY: false }, 24, 16)}
				<button
					type="button"
					class="tool"
					class:thumb={p !== 'off'}
					role="radio"
					aria-checked={composition.preset === p}
					aria-label={COMPOSITION_NAMES[p]}
					title={p === 'safe'
						? 'Safe areas: action (90%) and title (80%)'
						: p === 'off'
							? 'No composition guide'
							: COMPOSITION_NAMES[p]}
					onclick={() => opts.setComposition({ preset: p })}
				>
					{#if p === 'off'}
						Off
					{:else}
						<svg viewBox="-1 -1 26 18" width="26" height="18" aria-hidden="true">
							<rect x="0" y="0" width="24" height="16" />
							{#each shapes.lines as [x1, y1, x2, y2], i (i)}
								<line {x1} {y1} {x2} {y2} />
							{/each}
							{#if shapes.arcs.length}<path d={arcsPath(shapes.arcs)} />{/if}
						</svg>
					{/if}
				</button>
			{/each}
			{#each projectOwn ? [...mine, projectOwn] : mine as d (d.id)}
				{@const shapes = compositionShapes(
					{ preset: 'custom', custom: d, flipX: false, flipY: false },
					24,
					16
				)}
				<button
					type="button"
					class="tool thumb mine"
					role="radio"
					aria-checked={usingOwn(d)}
					aria-label={d.name}
					title={d === projectOwn ? `${d.name} (kept in this project)` : d.name}
					onclick={() => useOwn(d)}
				>
					<svg viewBox="-1 -1 26 18" width="26" height="18" aria-hidden="true">
						<rect x="0" y="0" width="24" height="16" />
						{#each shapes.lines as [x1, y1, x2, y2], i (i)}
							<line {x1} {y1} {x2} {y2} />
						{/each}
						{#if shapes.arcs.length}<path d={arcsPath(shapes.arcs)} />{/if}
						{#each shapes.ellipses as e, i (i)}
							<ellipse cx={e.c[0]} cy={e.c[1]} rx={e.rx} ry={e.ry} />
						{/each}
					</svg>
				</button>
			{/each}
		</div>
		{#if composition.preset === 'custom' && composition.custom}
			{@const own = composition.custom}
			<button
				type="button"
				class="tool"
				title="Change this guide in the composition creator"
				onclick={() => create(mine.find((d) => d.id === own.id) ?? own)}
			>
				Edit “{own.name}”
			</button>
		{/if}
		{#if compositionFlips(composition.preset) === 'x'}
			<button
				type="button"
				class="tool"
				aria-pressed={composition.flipX}
				title="Flip the guide left to right"
				onclick={() => opts.setComposition({ flipX: !composition.flipX })}
			>
				Flip
			</button>
		{:else if compositionFlips(composition.preset) === 'xy'}
			<button
				type="button"
				class="tool"
				aria-pressed={composition.flipX}
				aria-label="Flip left to right"
				title="Flip the guide left to right"
				onclick={() => opts.setComposition({ flipX: !composition.flipX })}
			>
				Flip ↔
			</button>
			<button
				type="button"
				class="tool"
				aria-pressed={composition.flipY}
				aria-label="Flip top to bottom"
				title="Flip the guide top to bottom"
				onclick={() => opts.setComposition({ flipY: !composition.flipY })}
			>
				Flip ↕
			</button>
		{/if}
		{#if compositionOn(opts.guides)}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'guides'}
				title="Drag the guide's middle to move it, its corner to size it"
				onclick={() => (tool = tool === 'guides' ? 'brush' : 'guides')}
			>
				Move guides
			</button>
		{/if}
		{#if compositionPlaced(composition)}
			<button
				type="button"
				class="tool"
				title="Put the guide back to filling the canvas"
				onclick={() => opts.recentreComposition()}
			>
				Re-centre
			</button>
		{/if}
		{#if composition.flipX || composition.flipY || compositionPlaced(composition)}
			<button
				type="button"
				class="tool"
				title="Put the guide back the way it started: not flipped, filling the canvas"
				onclick={() =>
					opts.setComposition({ flipX: false, flipY: false, offset: [0, 0], scale: 1 })}
			>
				Reset
			</button>
		{/if}
		<button
			type="button"
			class="tool"
			title="Make a composition guide of your own"
			onclick={() => create(null)}
		>
			New composition…
		</button>
		<button
			type="button"
			class="tool"
			title="Bring in composition guides from .ppcomp.json files"
			onclick={() => compFiles?.click()}
		>
			Import…
		</button>
		<input
			bind:this={compFiles}
			class="visually-hidden"
			type="file"
			multiple
			accept=".json,application/json"
			tabindex="-1"
			aria-label="Composition files to import"
			onchange={(e) => {
				const list = [...(e.currentTarget.files ?? [])];
				e.currentTarget.value = '';
				void importComps(list, false);
			}}
		/>
		{#if compNote}<span class="msg" role="status">{compNote}</span>{/if}
	</div>
{/if}
{#if creator}
	{#key creator.n}
		<CompositionCreator
			start={creator.start}
			canvasSize={opts.canvasSize}
			{askToDelete}
			ondone={(saved, deleted) => {
				creator = null;
				if (saved) useOwn(saved);
				else if (deleted && composition.custom?.id === deleted)
					opts.setComposition({ preset: 'off' });
			}}
			onimport={(list) => importComps(list, true)}
			onexport={async (name, text) => {
				const at = await opts.saveFile(name, text);
				return at ? `Exported as ${at.split(/[\\/]/).pop()}.` : '';
			}}
		/>
	{/key}
{/if}
{#if opts.adjustMsg}<p class="adjust-msg" role="alert">{opts.adjustMsg}</p>{/if}
{#if opts.importNote}<p class="adjust-msg" role="alert">{opts.importNote}</p>{/if}
{#if opts.adjustOpen}
	<div class="fill-opts adjust-opts" role="group" aria-label="Adjust layer">
		<div class="seg" role="radiogroup" aria-label="Adjustment">
			{#each [['colour', 'Colour'], ['curves', 'Curves'], ['levels', 'Levels'], ['map', 'Gradient map'], ...(opts.adjustLayer ? [] : [['filters', 'Filters']])] as [id, label] (id)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={opts.adjustKind === id}
					onclick={() => {
						// filters show on their own: switching to or from them shows the other again
						const swap = (id === 'filters') !== (opts.adjustKind === 'filters');
						opts.adjustKind = id as typeof opts.adjustKind;
						if (swap) opts.previewAdjust();
						if (id === 'map' && !opts.tone.map) {
							opts.tone = { ...opts.tone, map: MAP_PRESETS[0].stops };
							opts.previewAdjust();
						}
					}}
				>
					{label}
				</button>
			{/each}
		</div>
		{#if opts.adjustKind === 'colour'}
			{#each [['hue', 'Hue', -180, 180, 1, '°'], ['saturation', 'Saturation', -100, 100, 1, '%'], ['brightness', 'Brightness', -100, 100, 1, '%'], ['contrast', 'Contrast', -100, 100, 1, '%'], ['blur', 'Blur', 0, 40, 1, ' px']] as [key, label, min, max, st, u] (key)}
				{@const k = key as keyof typeof opts.adjust}
				{@const scale = k === 'hue' || k === 'blur' ? 1 : 100}
				{#if !(k === 'blur' && opts.adjustLayer)}
					<label class="slider">
						<span>{label} <b>{Math.round(opts.adjust[k] * scale)}{u}</b></span>
						<input
							type="range"
							{min}
							{max}
							step={st}
							value={Math.round(opts.adjust[k] * scale)}
							oninput={(e) => {
								opts.adjust = { ...opts.adjust, [k]: +e.currentTarget.value / scale };
								opts.previewAdjust();
							}}
						/>
					</label>
				{/if}
			{/each}
			{#if !opts.adjustLayer}
				<button
					type="button"
					class="tool"
					aria-pressed={opts.recolour}
					title="Paint the whole layer in the current colour (keeps its shading by transparency)"
					onclick={() => {
						opts.recolour = !opts.recolour;
						opts.previewAdjust();
					}}
				>
					Recolour
				</button>
			{/if}
		{:else if opts.adjustKind === 'filters'}
			<FilterEditor bind:spec={opts.filter} onchange={() => opts.previewAdjust()} />
		{:else}
			<ToneEditor
				kind={opts.adjustKind}
				bind:spec={opts.tone}
				onchange={() => opts.previewAdjust()}
			/>
		{/if}
		{#if (opts.adjustLayerNew || !opts.adjustLayer) && opts.adjustKind !== 'filters'}
			<button
				type="button"
				class="tool"
				aria-pressed={!!opts.adjustLayer}
				title="Keep editable: put the change on its own adjustment layer, which changes everything below it and can be changed or hidden later"
				onclick={() => opts.setKeepEditable(!opts.adjustLayer)}
			>
				Keep editable
			</button>
		{/if}
		<button type="button" class="tool done" onclick={() => opts.closeAdjust(true)}>Apply</button>
		<button type="button" class="tool" onclick={() => opts.closeAdjust(false)}>Cancel</button>
	</div>
{/if}
{#if tool === 'gradient'}
	<div class="fill-opts" role="group" aria-label="Gradient">
		<div class="seg" role="radiogroup" aria-label="Gradient shape">
			{#each [['linear', 'Straight'], ['radial', 'Round']] as [id, label] (id)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={opts.gradientKind === id}
					onclick={() => (opts.gradientKind = id as 'linear' | 'radial')}
				>
					{label}
				</button>
			{/each}
		</div>
		<div class="seg" role="radiogroup" aria-label="Gradient end">
			<button
				type="button"
				class="tool"
				role="radio"
				aria-checked={opts.gradientEnd === 'transparent'}
				onclick={() => (opts.gradientEnd = 'transparent')}
			>
				To transparent
			</button>
			<button
				type="button"
				class="tool"
				role="radio"
				aria-checked={opts.gradientEnd !== 'transparent'}
				onclick={() => (opts.gradientEnd = 'colour')}
			>
				To a colour
			</button>
		</div>
		{#if opts.gradientEnd !== 'transparent'}
			<label class="end-colour">
				<input type="color" bind:value={opts.gradientEndColor} aria-label="End colour" />
			</label>
		{/if}
		<span class="opt-label">Drag across the canvas</span>
	</div>
{/if}
{#if tool === 'liquify'}
	<div class="fill-opts" role="group" aria-label="Liquify">
		<label class="slider">
			<span>Size <b>{opts.liquifySize}</b></span>
			<input type="range" min="10" max="200" step="2" bind:value={opts.liquifySize} />
		</label>
		<label class="slider">
			<span>Strength <b>{Math.round(opts.liquifyStrength * 100)}%</b></span>
			<input type="range" min="0.1" max="1" step="0.05" bind:value={opts.liquifyStrength} />
		</label>
	</div>
{/if}
{#if tool === 'transform' && !pickedElsewhere && strokesShown}
	<div class="fill-opts move-hint" role="group" aria-label="Move">
		<span
			class="msg"
			data-testid="move-strokes-hint"
			title="Shift: move straight across, down or at 45°"
		>
			Tap a stroke to change it, or drag it. Drag anywhere else to move the whole layer.
		</span>
	</div>
{:else if tool === 'transform' && !pickedElsewhere}
	<div
		class="fill-opts"
		role="group"
		aria-label="Transform"
		title="Shift: move straight across, down or at 45°, turn in 15° steps, a corner scales each way on its own"
	>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.distort}
			title="Distort: drag each corner on its own (perspective, skew). Or hold Ctrl and drag a corner."
			onclick={() => {
				opts.distort = !opts.distort;
				if (opts.distort) opts.warp = false;
			}}
		>
			Distort
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.warp}
			title="Warp: bend it with a grid of 16 points"
			onclick={() => {
				opts.warp = !opts.warp;
				if (opts.warp) opts.distort = false;
			}}
		>
			Warp
		</button>
		<button type="button" class="tool" onclick={() => canvas?.transformFlip('x')}> Flip ↔ </button>
		<button type="button" class="tool" onclick={() => canvas?.transformFlip('y')}> Flip ↕ </button>
		<button type="button" class="tool" onclick={() => canvas?.transformRotate(90)}>
			Rotate 90°
		</button>
		<button type="button" class="tool" onclick={() => canvas?.transformReset()}> Reset </button>
		<button type="button" class="tool done" onclick={() => canvas?.transformCommit()}>
			Done
		</button>
		<button type="button" class="tool" onclick={() => canvas?.transformCancel()}> Cancel </button>
		{#if strokes}
			<span class="msg" data-testid="transform-strokes-note">
				Strokes stay editable when moved, scaled, turned or flipped. Distort and Warp make them
				plain pixels.
			</span>
		{/if}
	</div>
{/if}
{#if tool.startsWith('select') || status.selection}
	<div class="fill-opts" role="group" aria-label="Selection">
		{#if tool.startsWith('select')}
			<div class="seg" role="radiogroup" aria-label="Selection mode">
				{#each [['replace', 'New'], ['add', 'Add'], ['subtract', 'Subtract'], ['intersect', 'Intersect']] as [id, label] (id)}
					<button
						type="button"
						class="tool"
						role="radio"
						aria-checked={opts.selectMode === id}
						onclick={() => (opts.selectMode = id as typeof opts.selectMode)}
					>
						{label}
					</button>
				{/each}
			</div>
			<button
				type="button"
				class="tool"
				aria-pressed={opts.selectFill}
				title="Colour fill: each selection fills with the colour as you make it. Shapes you add, or another colour, change the fill until you deselect."
				onclick={() => (opts.selectFill = !opts.selectFill)}
			>
				Colour fill
			</button>
		{/if}
		{#if tool === 'select-rect'}
			<div class="seg" role="radiogroup" aria-label="Selection shape">
				{#each [['rect', 'Rectangle', 'Shift: keep it square'], ['ellipse', 'Ellipse', 'Shift: keep it round']] as [id, label, title] (id)}
					<button
						type="button"
						class="tool"
						role="radio"
						{title}
						aria-checked={opts.selectShape === id}
						onclick={() => (opts.selectShape = id as typeof opts.selectShape)}
					>
						{label}
					</button>
				{/each}
			</div>
		{/if}
		{#if tool === 'select-lasso'}
			<span class="adjust-msg plain">
				Drag to draw, or click point by point (click the first point or press Enter to close).
			</span>
		{/if}
		{#if tool === 'select-wand'}
			<label class="slider">
				<span>Tolerance <b>{Math.round(opts.wandTolerance * 100)}%</b></span>
				<input type="range" min="0" max="0.6" step="0.02" bind:value={opts.wandTolerance} />
			</label>
			<button
				type="button"
				class="tool"
				aria-pressed={!opts.wandContiguous}
				title="Colour range: every pixel of a similar colour, joined up or not"
				onclick={() => (opts.wandContiguous = !opts.wandContiguous)}
			>
				Colour range
			</button>
			<button
				type="button"
				class="tool"
				aria-pressed={opts.wandAllLayers}
				title="Look at what you see (all layers), or only this layer's own pixels"
				onclick={() => (opts.wandAllLayers = !opts.wandAllLayers)}
			>
				All layers
			</button>
			<button
				type="button"
				class="tool"
				title="Select the current colour wherever it is (Colour range with your colour)"
				onclick={() => canvas?.selectColour()}
			>
				Select colour
			</button>
		{/if}
		<button type="button" class="tool" onclick={() => canvas?.selectAll()}>All</button>
		<button
			type="button"
			class="tool"
			title="Select what's on this layer (or Ctrl+click a layer's picture in Layers)"
			onclick={() => canvas?.selectLayerContents()}
		>
			Layer contents
		</button>
		<button type="button" class="tool" title="Copy (Ctrl+C)" onclick={() => opts.copyCut(false)}>
			Copy
		</button>
		<button type="button" class="tool" title="Cut (Ctrl+X)" onclick={() => opts.copyCut(true)}>
			Cut
		</button>
		<button
			type="button"
			class="tool"
			title="Paste as a new layer, then move it (Ctrl+V)"
			onclick={() => opts.pasteCopied()}
		>
			Paste
		</button>
		{#if status.selection}
			<label class="slider">
				<span>Feather <b>{opts.featherPx}px</b></span>
				<input type="range" min="2" max="80" step="1" bind:value={opts.featherPx} />
			</label>
			<button
				type="button"
				class="tool"
				title="Soften the selection's edge, for gentle shading inside it"
				onclick={() => canvas?.featherSelection(opts.featherPx)}
			>
				Feather
			</button>
			<label class="slider">
				<span>Grow / shrink <b>{opts.growPx}px</b></span>
				<input type="range" min="1" max="100" step="1" bind:value={opts.growPx} />
			</label>
			<button
				type="button"
				class="tool"
				title="Make the selection bigger all round (Expand)"
				onclick={() => canvas?.growSelection(opts.growPx)}
			>
				Grow
			</button>
			<button
				type="button"
				class="tool"
				title="Make the selection smaller all round (Contract)"
				onclick={() => canvas?.growSelection(-opts.growPx)}
			>
				Shrink
			</button>
			<button
				type="button"
				class="tool"
				title="Fill what's selected with the colour"
				onclick={() => canvas?.fillSelection()}
			>
				Fill
			</button>
			<button type="button" class="tool" onclick={() => canvas?.deselect()}>None</button>
			<button type="button" class="tool" onclick={() => canvas?.invertSelection()}> Invert </button>
			<button type="button" class="tool" onclick={() => canvas?.clear()}> Clear selected </button>
		{/if}
	</div>
{/if}
{#if tool === 'fill'}
	<div class="fill-opts" role="group" aria-label="Fill options">
		<label class="slider">
			<span>Tolerance <b>{Math.round(opts.fillTolerance * 100)}%</b></span>
			<input type="range" min="0" max="0.6" step="0.02" bind:value={opts.fillTolerance} />
		</label>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.fillAllLayers}
			title="Fill what you see (all layers) or only this layer's own pixels"
			onclick={() => (opts.fillAllLayers = !opts.fillAllLayers)}
			disabled={!!opts.fillReference}
		>
			{opts.fillAllLayers ? 'Sample: all layers' : 'Sample: this layer'}
		</button>
		<label class="ref-layer">
			<span>Lines from</span>
			<select
				bind:value={opts.fillRefLayer}
				title="Fill inside the lines of a line-art layer while you colour on another layer"
			>
				<option value={0}>{opts.fillAllLayers ? 'What you see' : 'This layer'}</option>
				{#each [...status.layers].reverse() as l (l.id)}
					<option value={l.id}>Reference: {l.name}</option>
				{/each}
			</select>
		</label>
		{#if opts.animated}
			<button
				type="button"
				class="tool"
				aria-pressed={opts.fillAcross}
				disabled={!opts.fillReference}
				title={opts.fillReference
					? 'Each fill carries on through the next frames: the same shape in each line drawing gets the colour'
					: 'Pick the line layer under “Lines from” first'}
				onclick={() => (opts.fillAcross = !opts.fillAcross)}
			>
				Carry to next frames
			</button>
			{#if opts.setupColour}
				<button
					type="button"
					class="tool"
					title="A Colour layer under this line layer (fills take its lines) and a Shadow layer clipped to it"
					onclick={() => opts.setupColour?.()}
				>
					Set up colour layers
				</button>
			{/if}
		{/if}
	</div>
{/if}
{#if tool === 'smudge'}
	<div class="fill-opts" role="group" aria-label="Smudge">
		<label class="slider">
			<span>Strength <b>{Math.round(opts.smudgeStrength * 100)}%</b></span>
			<input type="range" min="0" max="1" step="0.05" bind:value={opts.smudgeStrength} />
		</label>
		<span class="adjust-msg plain">Smudges with the shape of the brush you picked.</span>
	</div>
{/if}
{#if tool === 'clone'}
	<div class="fill-opts" role="group" aria-label="Clone">
		<button
			type="button"
			class="tool"
			title="Tap the canvas next to set where to copy from (or Alt+click it)"
			onclick={() => canvas?.cloneSetSource()}
		>
			Set source
		</button>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.cloneAligned}
			title="Aligned: every stroke keeps the same distance to the source. Off: each stroke starts at the source again."
			onclick={() => (opts.cloneAligned = !opts.cloneAligned)}
		>
			Aligned
		</button>
		<div class="seg" role="radiogroup" aria-label="Copy from">
			{#each [[false, 'This layer'], [true, 'All layers']] as const as [all, label] (label)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={opts.cloneAllLayers === all}
					onclick={() => (opts.cloneAllLayers = all)}
				>
					{label}
				</button>
			{/each}
		</div>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.cloneHeal}
			title="Heal: what's copied takes on the colour and brightness around where you paint (for mending spots)"
			onclick={() => (opts.cloneHeal = !opts.cloneHeal)}
		>
			Heal
		</button>
		<span class="adjust-msg plain">
			Alt+click where to copy from, then paint with the brush you picked.
		</span>
	</div>
{/if}
{#if tool === 'pen'}
	<div class="fill-opts" role="group" aria-label="Pen path">
		<span class="adjust-msg plain">
			{opts.penInfo.points === 0
				? 'Click for corners, drag for curves. Click the first point to close the shape.'
				: `${opts.penInfo.points} point${opts.penInfo.points === 1 ? '' : 's'}${opts.penInfo.closed ? ', closed' : ''}. Drag points or handles to change it (Alt breaks a handle).`}
		</span>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.penTaper}
			title="Thinner at the ends, like a hand-drawn line"
			onclick={() => (opts.penTaper = !opts.penTaper)}
		>
			Taper
		</button>
		<button
			type="button"
			class="tool done"
			disabled={opts.penInfo.points < 2}
			title="Draw along the path with the current brush (Enter)"
			onclick={() => {
				if (canvas?.penStroke(opts.penTaper)) canvas.penClear();
			}}
		>
			Draw line
		</button>
		<button
			type="button"
			class="tool"
			disabled={opts.penInfo.points < 3}
			title="Fill inside the path with the colour"
			onclick={() => {
				if (canvas?.penFill()) canvas.penClear();
			}}
		>
			Fill
		</button>
		<button
			type="button"
			class="tool"
			disabled={!opts.penInfo.points}
			title="Take back the last point (Backspace)"
			onclick={() => canvas?.penBack()}
		>
			Back
		</button>
		<button
			type="button"
			class="tool"
			disabled={!opts.penInfo.points}
			onclick={() => canvas?.penClear()}
		>
			Clear
		</button>
	</div>
{/if}
{#if tool === 'measure'}
	<div class="fill-opts" role="group" aria-label="Measure">
		<span class="adjust-msg plain">
			{measureUnit
				? 'Drag across your drawing: the ticks are your unit from the reference.'
				: 'Drag to measure in pixels. Mark a unit on the reference (the ruler button under it) to measure in units.'}
		</span>
	</div>
{/if}
<TextOptions text={opts.text} {tool} {status} />
{#if tool === 'lasso-fill'}
	<div class="fill-opts" role="group" aria-label="Lasso fill">
		<div class="seg" role="radiogroup" aria-label="Lasso fill mode">
			{#each [['fill', 'Fill', 'Fill the shape with the colour'], ['erase', 'Erase', 'Cut the shape out of the layer (or hold Alt as you finish)']] as const as [id, label, title] (id)}
				<button
					type="button"
					class="tool"
					role="radio"
					{title}
					aria-checked={opts.lassoMode === id}
					onclick={() => (opts.lassoMode = id)}
				>
					{label}
				</button>
			{/each}
		</div>
		<label class="slider" title="Smooths the freehand parts of the shape; corners stay sharp">
			<span>Smoothing <b>{Math.round(opts.lassoSmoothing * 100)}%</b></span>
			<input type="range" min="0" max="1" step="0.05" bind:value={opts.lassoSmoothing} />
		</label>
		<button
			type="button"
			class="tool"
			aria-pressed={opts.lassoHard}
			title="Hard edges: no soft (anti-aliased) pixels along the edge"
			onclick={() => (opts.lassoHard = !opts.lassoHard)}
		>
			Hard edges
		</button>
		<span class="adjust-msg plain">
			Drag to draw, click for straight edges (click the first point, double-click or press Enter to
			close). Hold Alt to cut instead.
		</span>
	</div>
{/if}

<style>
	.msg {
		margin: 0;
		color: var(--pp-on-ink);
		font-size: 14px;
	}
	.tool {
		min-width: 38px;
		height: 38px;
		padding: 0 12px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tool[aria-pressed='true'],
	.tool[role='radio'][aria-checked='true'],
	.tool.done {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.tool:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.seg {
		display: flex;
		gap: 2px;
	}
	/* the composition picker: a small drawing of each guide */
	.tool.thumb {
		padding: 0 6px;
	}
	.thumb svg {
		display: block;
		fill: none;
		stroke: currentColor;
		stroke-width: 1;
	}
	.thumb svg rect {
		stroke-opacity: 0.6;
	}
	/* one of My compositions: a dot under it */
	.tool.mine {
		position: relative;
	}
	.tool.mine::after {
		content: '';
		position: absolute;
		bottom: 2px;
		left: 50%;
		width: 4px;
		height: 4px;
		margin-left: -2px;
		border-radius: 50%;
		background: currentColor;
		opacity: 0.6;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
	.slider {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		color: var(--pp-on-ink-muted);
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
	}
	.ref-layer {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-on-ink);
	}
	.ref-layer select {
		height: 38px;
		max-width: 220px;
		padding: 0 10px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
	}
	.adjust-msg {
		margin: 0;
		color: #ffd6cc;
		font-weight: 600;
		font-size: 14px;
	}
	.adjust-msg.plain {
		color: var(--pp-on-ink);
		font-weight: 500;
	}
	.end-colour input {
		width: 44px;
		height: 36px;
		padding: 0;
		border: 0;
		border-radius: 10px;
		background: transparent;
		cursor: pointer;
	}
	.opt-label {
		font-size: 13px;
		font-weight: 600;
		color: rgba(255, 255, 255, 0.75);
	}
	/* always one compact row (scrolls sideways when the window is narrow) */
	.fill-opts {
		display: flex;
		flex-wrap: nowrap;
		align-items: center;
		gap: 8px 12px;
		min-width: 0;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.fill-opts > * {
		flex: none;
	}
	/* the Composition row grows with your own guides: it wraps, nothing goes off the edge */
	.fill-opts.comp-opts {
		flex-wrap: wrap;
		overflow-x: visible;
	}
	.fill-opts.comp-opts > .seg {
		flex: 0 1 auto;
		flex-wrap: wrap;
	}
	/* (as tall as a row of buttons: the canvas stays put when Move's box comes) */
	.move-hint {
		min-height: 38px;
	}
	.fill-opts .slider {
		flex: none;
		width: 130px;
		min-width: 0;
	}
</style>
