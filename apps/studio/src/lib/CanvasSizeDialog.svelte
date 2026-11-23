<script lang="ts">
	/**
	 * Image → Canvas size: a new width and height (in pixels, cm or inches), presets, a
	 * locked aspect ratio, and either where the drawing sits on the new canvas (the anchor:
	 * more room, or a crop) or the drawing resized with it. A small picture shows the new
	 * frame against the old one. Every frame of every layer changes, in one undo step.
	 */
	import {
		CANVAS_PRESETS,
		presetOf,
		sizeNote,
		type CanvasSize
	} from '@pencilpilot/paint/canvas-size';
	import {
		ANCHORS,
		DEFAULT_PPI,
		UNITS,
		checkResize,
		fromPx,
		keepAspect,
		planResize,
		toPx,
		type Anchor,
		type ResizePlan,
		type Unit
	} from './canvas-resize';

	let {
		size,
		tiles,
		limitBytes,
		picture = null,
		onapply,
		onclose
	}: {
		/** a small picture of the drawing as it shows (an image URL), for the preview */
		picture?: string | null;
		/** the canvas as it is */
		size: CanvasSize;
		/** about how many 64×64 tiles of paint the project has (for the memory check) */
		tiles: number;
		/** the frame memory limit (Settings) */
		limitBytes: number;
		onapply: (plan: ResizePlan) => void;
		onclose: () => void;
	} = $props();

	// (the dialog opens on the size the canvas is; it doesn't follow it while open)
	const start = (() => ({ ...size }))();
	let width = $state(start.width);
	let height = $state(start.height);
	let unit = $state<Unit>('px');
	let ppi = $state(DEFAULT_PPI);
	let locked = $state(true);
	let scale = $state(false);
	let anchor = $state<Anchor>([0.5, 0.5]);

	const preset = $derived(presetOf({ width, height }));
	function pickPreset(id: string) {
		const p = CANVAS_PRESETS.find((x) => x.id === id);
		if (p) [width, height] = [p.width, p.height];
	}
	/** a side typed in the chosen unit */
	function setSide(side: 'width' | 'height', v: number) {
		if (!Number.isFinite(v) || v <= 0) return;
		const px = toPx(v, unit, ppi);
		if (locked) ({ width, height } = keepAspect({ width, height }, side, px));
		else if (side === 'width') width = px;
		else height = px;
	}
	const shown = (px: number) => fromPx(px, unit, ppi);

	const plan = $derived(planResize(start, { width, height }, { scale, anchor }));
	const check = $derived(checkResize(plan, tiles, limitBytes));
	const note = $derived(check.ok ? (check.note ?? sizeNote({ width, height })) : null);

	function anchorName([ax, ay]: Anchor) {
		if (ax === 0.5 && ay === 0.5) return 'the middle';
		const v = ay === 0 ? 'top' : ay === 1 ? 'bottom' : '';
		const h = ax === 0 ? 'left' : ax === 1 ? 'right' : '';
		return `the ${[v, h].filter(Boolean).join(' ')}`;
	}

	// ---- the preview: the old canvas (where the drawing is) and the new one, to scale ----
	const PREVIEW = 150;
	const view = $derived.by(() => {
		const { dx, dy } = plan;
		const old = scale
			? { x: 0, y: 0, w: width, h: height }
			: { x: dx, y: dy, w: start.width, h: start.height };
		const x0 = Math.min(0, old.x);
		const y0 = Math.min(0, old.y);
		const x1 = Math.max(width, old.x + old.w);
		const y1 = Math.max(height, old.y + old.h);
		const k = PREVIEW / Math.max(x1 - x0, y1 - y0, 1);
		const box = (r: { x: number; y: number; w: number; h: number }) => ({
			x: (r.x - x0) * k,
			y: (r.y - y0) * k,
			w: Math.max(1, r.w * k),
			h: Math.max(1, r.h * k)
		});
		return {
			w: (x1 - x0) * k,
			h: (y1 - y0) * k,
			old: box(old),
			now: box({ x: 0, y: 0, w: width, h: height })
		};
	});

	let dialog = $state<HTMLDialogElement>();
	$effect(() => {
		if (dialog && !dialog.open) dialog.showModal();
	});
	function apply() {
		if (check.ok) onapply(plan);
	}
</script>

<dialog
	bind:this={dialog}
	class="canvas-size"
	aria-label="Canvas size"
	oncancel={(e) => {
		e.preventDefault();
		onclose();
	}}
>
	<h2>Canvas size</h2>
	<p class="now">
		Now {start.width} × {start.height} px. Every frame of every layer changes with it.
	</p>
	<div class="body">
		<div class="fields">
			<label class="wide">
				<span>Preset</span>
				<select
					value={preset}
					onchange={(e) => pickPreset(e.currentTarget.value)}
					aria-label="Preset"
				>
					{#each CANVAS_PRESETS as p (p.id)}
						<option value={p.id}>{p.label} ({p.width} × {p.height})</option>
					{/each}
					<option value="custom" disabled>Custom</option>
				</select>
			</label>
			<div class="sides">
				<label>
					<span>Width</span>
					<input
						type="number"
						min="1"
						step={unit === 'px' ? 1 : 0.01}
						value={shown(width)}
						onchange={(e) => setSide('width', e.currentTarget.valueAsNumber)}
						aria-label="Width"
					/>
				</label>
				<button
					type="button"
					class="lock"
					aria-pressed={locked}
					title={locked ? 'Aspect ratio locked' : 'Aspect ratio free'}
					aria-label="Lock aspect ratio"
					onclick={() => (locked = !locked)}
				>
					<svg viewBox="0 0 24 24" aria-hidden="true">
						{#if locked}
							<path
								d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"
							/>
						{:else}
							<path
								d="m18.8 13.4 1.7-1.7a5 5 0 0 0-7-7l-1.7 1.7M5.2 10.6l-1.7 1.7a5 5 0 0 0 7 7l1.7-1.7M8 2v3M2 8h3M16 22v-3M22 16h-3"
							/>
						{/if}
					</svg>
				</button>
				<label>
					<span>Height</span>
					<input
						type="number"
						min="1"
						step={unit === 'px' ? 1 : 0.01}
						value={shown(height)}
						onchange={(e) => setSide('height', e.currentTarget.valueAsNumber)}
						aria-label="Height"
					/>
				</label>
				<label>
					<span>Unit</span>
					<select bind:value={unit} aria-label="Unit">
						{#each UNITS as u (u.id)}
							<option value={u.id}>{u.label}</option>
						{/each}
					</select>
				</label>
			</div>
			{#if unit !== 'px'}
				<label class="ppi">
					<span>at</span>
					<input type="number" min="36" max="2400" bind:value={ppi} aria-label="Pixels per inch" />
					<span>pixels per inch ({width} × {height} px)</span>
				</label>
			{/if}
			<label class="check">
				<input type="checkbox" bind:checked={scale} />
				Resize the drawing too (scale it with the canvas)
			</label>
			<div class="anchor-row" class:off={scale}>
				<div class="anchor" role="radiogroup" aria-label="Anchor">
					{#each ANCHORS as a (a.join())}
						<button
							type="button"
							role="radio"
							aria-checked={anchor[0] === a[0] && anchor[1] === a[1]}
							aria-label="Keep {anchorName(a)} in place"
							disabled={scale}
							onclick={() => (anchor = a)}
						></button>
					{/each}
				</div>
				<p>
					{#if scale}
						The drawing is stretched to fill the new canvas.
					{:else}
						The drawing keeps its size; {anchorName(anchor)} stays in place. Room is added, or the edges
						cropped, around it.
					{/if}
				</p>
			</div>
		</div>
		<figure class="preview" aria-label="Preview">
			<svg
				viewBox="-2 -2 {view.w + 4} {view.h + 4}"
				width={view.w + 4}
				height={view.h + 4}
				aria-hidden="true"
			>
				<defs>
					<clipPath id="canvas-size-clip">
						<rect x={view.now.x} y={view.now.y} width={view.now.w} height={view.now.h} />
					</clipPath>
				</defs>
				<rect class="new" x={view.now.x} y={view.now.y} width={view.now.w} height={view.now.h} />
				{#if picture}
					<!-- the drawing: faint where it's cropped off, clear on the new canvas -->
					<image
						href={picture}
						x={view.old.x}
						y={view.old.y}
						width={view.old.w}
						height={view.old.h}
						preserveAspectRatio="none"
						opacity="0.3"
					/>
					<image
						href={picture}
						x={view.old.x}
						y={view.old.y}
						width={view.old.w}
						height={view.old.h}
						preserveAspectRatio="none"
						clip-path="url(#canvas-size-clip)"
					/>
				{/if}
				<rect class="old" x={view.old.x} y={view.old.y} width={view.old.w} height={view.old.h} />
				<rect class="edge" x={view.now.x} y={view.now.y} width={view.now.w} height={view.now.h} />
			</svg>
			<figcaption>
				<span><span class="key old"></span>the drawing now</span>
				<span><span class="key new"></span>the new canvas</span>
			</figcaption>
		</figure>
	</div>
	{#if !check.ok}
		<p class="why" role="alert">{check.why}</p>
	{:else if note}
		<p class="note">{note}</p>
	{/if}
	<div class="actions">
		<button type="button" onclick={onclose}>Cancel</button>
		<button type="button" class="go" disabled={!check.ok} onclick={apply}>Resize canvas</button>
	</div>
</dialog>

<style>
	.canvas-size {
		width: min(600px, calc(100vw - 32px));
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.canvas-size::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 4px;
		font: 700 20px var(--pp-font-display);
	}
	.now {
		margin: 0 0 14px;
		color: var(--pp-muted);
		font-size: 13px;
	}
	.body {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		gap: 18px;
		align-items: start;
	}
	.fields {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 12px;
		font-weight: 600;
	}
	select,
	input[type='number'] {
		box-sizing: border-box;
		height: 34px;
		padding: 0 8px;
		border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
		border-radius: 10px;
		background: var(--pp-surface);
		color: inherit;
		font: inherit;
		font-weight: 400;
	}
	.sides {
		display: grid;
		grid-template-columns: 1fr auto 1fr 1fr;
		gap: 8px;
		align-items: end;
	}
	.sides input {
		width: 100%;
	}
	.lock {
		width: 34px;
		height: 34px;
		padding: 0;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
		color: inherit;
		cursor: pointer;
	}
	.lock[aria-pressed='true'] {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.lock svg {
		width: 20px;
		height: 20px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2;
		stroke-linecap: round;
	}
	.ppi {
		flex-direction: row;
		align-items: center;
		gap: 8px;
		font-weight: 400;
		color: var(--pp-muted);
	}
	.ppi input {
		width: 80px;
	}
	.check {
		flex-direction: row;
		align-items: center;
		gap: 8px;
		font-size: 14px;
		font-weight: 600;
	}
	.anchor-row {
		display: flex;
		gap: 12px;
		align-items: center;
	}
	.anchor-row p {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.anchor-row.off .anchor {
		opacity: 0.4;
	}
	.anchor {
		flex: none;
		display: grid;
		grid-template-columns: repeat(3, 26px);
		gap: 4px;
	}
	.anchor button {
		width: 26px;
		height: 26px;
		padding: 0;
		border: 2px solid color-mix(in srgb, currentColor 22%, transparent);
		border-radius: 6px;
		background: var(--pp-surface);
		cursor: pointer;
	}
	.anchor button[aria-checked='true'] {
		background: var(--pp-accent-strong, #e8492f);
		border-color: var(--pp-accent-strong, #e8492f);
	}
	.anchor button:disabled {
		cursor: not-allowed;
	}
	.preview {
		margin: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		width: 170px;
	}
	.preview svg {
		display: block;
	}
	/* (the paper is white in either theme) */
	rect.new {
		fill: #fff;
	}
	rect.edge {
		fill: none;
		stroke: var(--pp-accent-strong, #e8492f);
		stroke-width: 2;
	}
	rect.old {
		fill: rgba(90, 90, 104, 0.1);
		stroke: #8a8a96;
		stroke-width: 1.5;
		stroke-dasharray: 4 3;
	}
	figcaption {
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 12px;
		color: var(--pp-muted);
	}
	.key {
		display: inline-block;
		box-sizing: border-box;
		width: 12px;
		height: 10px;
		margin-right: 6px;
		vertical-align: -1px;
		border-radius: 2px;
	}
	.key.old {
		border: 1.5px dashed #8a8a96;
	}
	.key.new {
		border: 2px solid var(--pp-accent-strong, #e8492f);
		box-sizing: border-box;
	}
	.why,
	.note {
		margin: 14px 0 0;
		padding: 8px 10px;
		border-radius: 10px;
		font-size: 13px;
	}
	.why {
		background: color-mix(in srgb, #e8492f 16%, transparent);
	}
	.note {
		background: var(--pp-note);
	}
	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 16px;
	}
	.actions button {
		height: 38px;
		padding: 0 16px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-track);
		color: inherit;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.actions .go {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.actions button:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	@media (max-width: 560px) {
		.body {
			grid-template-columns: 1fr;
		}
		.preview {
			width: auto;
		}
	}
</style>
