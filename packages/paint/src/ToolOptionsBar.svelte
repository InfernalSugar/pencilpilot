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
	import { SYMMETRY_MODES, symmetryOn } from './guides';
	import { MAP_PRESETS } from './tone';
	import type { ToolOptions } from './tool-options.svelte';

	let {
		opts,
		tool = $bindable('brush'),
		status,
		canvas,
		measureUnit = 0
	}: {
		opts: ToolOptions;
		tool?: string;
		status: CanvasStatus;
		canvas: ReturnType<typeof PaintCanvas> | undefined;
		/** the unit marked on the reference, in canvas px (0: measure in pixels) */
		measureUnit?: number;
	} = $props();
</script>

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
					onclick={() =>
						opts.setGuides({
							...opts.guides,
							perspective: { ...opts.guides.perspective, points: n as 0 | 1 | 2 | 3 }
						})}
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
				title="Drawing assist on this layer: strokes snap to the grid, isometric or perspective directions (each layer has its own, in Layers)"
				onclick={() => layer && canvas?.setLayerAssist(layer.id, !layer.assist)}
			>
				Assist
			</button>
		{/if}
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
	</div>
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
{#if tool === 'transform'}
	<div class="fill-opts" role="group" aria-label="Transform">
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
				{#each [['rect', 'Rectangle'], ['ellipse', 'Ellipse']] as [id, label] (id)}
					<button
						type="button"
						class="tool"
						role="radio"
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
		<span class="adjust-msg plain">
			Draw a shape: it fills with the colour when you let go. Great for blocking in big shapes and
			shadows.
		</span>
	</div>
{/if}

<style>
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
	.fill-opts .slider {
		flex: none;
		width: 130px;
		min-width: 0;
	}
</style>
