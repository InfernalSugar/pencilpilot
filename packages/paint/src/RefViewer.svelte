<script lang="ts">
	/**
	 * The reference picture with its own zoom, pan and flip (it never changes the
	 * canvas), a colour picker, a grayscale view and a grid that matches the
	 * canvas grid (the grid method: copy one square at a time).
	 */
	let {
		src,
		alt,
		fill = false,
		gray = false,
		grid = null,
		onpick,
		notan = 0,
		squint = false,
		unit = null,
		angle = null,
		onunit,
		onangle
	}: {
		src: string;
		alt: string;
		fill?: boolean;
		/** show the picture in grey (values only) */
		gray?: boolean;
		/** grid segments [x1, y1, x2, y2] in fractions (0–1) of the picture */
		grid?: [number, number, number, number][] | null;
		/** a colour was picked from the picture (the eyedropper button, or Alt+click) */
		onpick?: (hex: string) => void;
		/** notan view: 2 or 3 flat tones (0 = off) */
		notan?: 0 | 2 | 3;
		/** squint view: blurred */
		squint?: boolean;
		/** the measuring unit marked on the picture (fractions of it), shown as "1 unit" */
		unit?: [Seg, Seg] | null;
		/** an angle marked on the picture (fractions of it) */
		angle?: [Seg, Seg] | null;
		/** a unit was marked (or cleared): the canvas's measuring tool counts in it */
		onunit?: (seg: [Seg, Seg] | null) => void;
		/** an angle was marked (or cleared): the canvas shows it as a guide */
		onangle?: (seg: [Seg, Seg] | null) => void;
	} = $props();

	type Seg = [number, number];
	/** drawing a unit or an angle line on the picture */
	let lineMode = $state<'' | 'unit' | 'angle'>('');
	let drawing = $state<[Seg, Seg] | null>(null);
	const fid = `pp-ref-notan-${Math.random().toString(36).slice(2, 9)}`;

	let zoom = $state(1);
	let panX = $state(0);
	let panY = $state(0);
	let flipped = $state(false);
	let picking = $state(false);
	let showGrid = $state(true);
	let pickNote = $state('');
	let natW = $state(0);
	let natH = $state(0);
	let box: HTMLDivElement;
	let img: HTMLImageElement;
	let drag: { id: number; x: number; y: number; px: number; py: number } | null = null;
	/** the grid's clip path needs an id of its own (the picture can be shown twice) */
	const clipId = `ref-clip-${Math.random().toString(36).slice(2, 9)}`;

	function zoomBy(f: number) {
		const z = Math.max(1, Math.min(8, zoom * f));
		panX *= z / zoom;
		panY *= z / zoom;
		zoom = z;
		if (zoom === 1) panX = panY = 0;
	}
	function reset() {
		zoom = 1;
		panX = panY = 0;
		flipped = false;
	}

	$effect(() => {
		const onWheel = (e: WheelEvent) => {
			if (!(e.ctrlKey || e.metaKey) && zoom === 1) return;
			e.preventDefault();
			if (e.ctrlKey || e.metaKey) zoomBy(Math.exp(-e.deltaY * 0.01));
			else {
				panX -= e.deltaX;
				panY -= e.deltaY;
			}
		};
		box.addEventListener('wheel', onWheel, { passive: false });
		return () => box.removeEventListener('wheel', onWheel);
	});

	/** The picture's pixels, read once (for the colour picker). */
	let pixels: ImageData | null = null;
	function readPixels(): ImageData | null {
		if (pixels) return pixels;
		if (!img?.naturalWidth) return null;
		const c = document.createElement('canvas');
		[c.width, c.height] = [img.naturalWidth, img.naturalHeight];
		const g = c.getContext('2d', { willReadFrequently: true })!;
		g.drawImage(img, 0, 0);
		try {
			pixels = g.getImageData(0, 0, c.width, c.height);
		} catch {
			pixels = null; // a picture from another site can't be read
		}
		return pixels;
	}

	/** The picture pixel under a screen point, undoing the zoom, pan, flip and "fit inside". */
	function pictureAt(clientX: number, clientY: number): [number, number] | null {
		if (!natW || !natH) return null;
		const r = box.getBoundingClientRect();
		const [cx, cy] = [r.left + r.width / 2, r.top + r.height / 2];
		// before the view transform (scale about the centre, then pan)
		const sx = zoom * (flipped ? -1 : 1);
		const lx = (clientX - cx - panX) / sx;
		const ly = (clientY - cy - panY) / zoom;
		const k = Math.min(r.width / natW, r.height / natH);
		const [x, y] = [lx / k + natW / 2, ly / k + natH / 2];
		return x >= 0 && y >= 0 && x < natW && y < natH ? [Math.floor(x), Math.floor(y)] : null;
	}

	/** The picture point under a screen point, as fractions (0–1) of the picture. */
	function fracAt(clientX: number, clientY: number): Seg | null {
		if (!natW || !natH) return null;
		const r = box.getBoundingClientRect();
		const [cx, cy] = [r.left + r.width / 2, r.top + r.height / 2];
		const lx = (clientX - cx - panX) / (zoom * (flipped ? -1 : 1));
		const ly = (clientY - cy - panY) / zoom;
		const k = Math.min(r.width / natW, r.height / natH);
		const c = (v: number) => Math.min(1, Math.max(0, v));
		return [c((lx / k + natW / 2) / natW), c((ly / k + natH / 2) / natH)];
	}

	/** an angle in degrees (0 = flat, counter-clockwise, 0–180) of a line in picture fractions */
	const degOf = ([a, b]: [Seg, Seg]) =>
		((((Math.atan2(-(b[1] - a[1]) * natH, (b[0] - a[0]) * natW) * 180) / Math.PI) % 180) + 180) %
		180;

	function pickAt(clientX: number, clientY: number) {
		const p = pictureAt(clientX, clientY);
		if (!p) return;
		const d = readPixels();
		if (!d) {
			pickNote = 'This picture’s colours can’t be read.';
			return;
		}
		// average a 3×3 patch: steadier than one pixel on photos
		let [r, g, b, n] = [0, 0, 0, 0];
		for (let y = Math.max(0, p[1] - 1); y <= Math.min(d.height - 1, p[1] + 1); y++)
			for (let x = Math.max(0, p[0] - 1); x <= Math.min(d.width - 1, p[0] + 1); x++) {
				const i = (y * d.width + x) * 4;
				[r, g, b, n] = [r + d.data[i], g + d.data[i + 1], b + d.data[i + 2], n + 1];
			}
		const hex = (v: number) =>
			Math.round(v / n)
				.toString(16)
				.padStart(2, '0');
		pickNote = '';
		onpick?.(`#${hex(r)}${hex(g)}${hex(b)}`);
	}

	/** grayscale / notan / squint, as one CSS filter */
	const imgFilter = $derived(
		[
			gray && !notan ? 'grayscale(1)' : '',
			notan ? `url(#${fid}-${notan})` : '',
			squint ? 'blur(4px)' : ''
		]
			.filter(Boolean)
			.join(' ') || 'none'
	);
	const view = $derived(
		`transform: translate(${panX}px, ${panY}px) scale(${zoom * (flipped ? -1 : 1)}, ${zoom})`
	);
</script>

<div class="ref" class:fill>
	<div
		class="box"
		class:zoomed={zoom > 1}
		class:picking
		class:lining={!!lineMode}
		bind:this={box}
		role="presentation"
		data-testid="ref-box"
		ondblclick={() => {
			if (picking) return;
			if (zoom > 1) reset();
			else zoomBy(2);
		}}
		onpointerdown={(e) => {
			if (lineMode) {
				const p = fracAt(e.clientX, e.clientY);
				if (!p) return;
				box.setPointerCapture(e.pointerId);
				drawing = [p, p];
				return;
			}
			if (onpick && (picking || e.altKey)) {
				e.preventDefault();
				pickAt(e.clientX, e.clientY);
				return;
			}
			if (zoom === 1) return;
			box.setPointerCapture(e.pointerId);
			drag = { id: e.pointerId, x: e.clientX, y: e.clientY, px: panX, py: panY };
		}}
		onpointermove={(e) => {
			if (drawing) {
				const p = fracAt(e.clientX, e.clientY);
				if (p) drawing = [drawing[0], p];
				return;
			}
			if (!drag || e.pointerId !== drag.id) return;
			panX = drag.px + e.clientX - drag.x;
			panY = drag.py + e.clientY - drag.y;
		}}
		onpointerup={() => {
			drag = null;
			if (!drawing) return;
			const seg = drawing;
			drawing = null;
			// a tap isn't a line
			if (Math.hypot((seg[1][0] - seg[0][0]) * natW, (seg[1][1] - seg[0][1]) * natH) < 4) return;
			if (lineMode === 'unit') onunit?.(seg);
			else if (lineMode === 'angle') onangle?.(seg);
			lineMode = '';
		}}
		onpointercancel={() => (drag = null)}
	>
		<img
			class="fade-in"
			bind:this={img}
			{src}
			{alt}
			draggable="false"
			style="{view}; filter: {imgFilter}"
			onload={() => {
				natW = img.naturalWidth;
				natH = img.naturalHeight;
				pixels = null;
			}}
		/>
		{#if grid?.length && showGrid && natW}
			<!-- the same "fit inside" as the picture, so the lines sit on it -->
			<svg
				class="grid"
				viewBox="0 0 {natW} {natH}"
				preserveAspectRatio="xMidYMid meet"
				style={view}
				aria-hidden="true"
				data-testid="ref-grid"
			>
				<defs>
					<clipPath id={clipId}><rect width={natW} height={natH} /></clipPath>
				</defs>
				<g clip-path="url(#{clipId})">
					{#each grid as [x1, y1, x2, y2], i (i)}
						<line x1={x1 * natW} y1={y1 * natH} x2={x2 * natW} y2={y2 * natH} />
					{/each}
				</g>
			</svg>
		{/if}
		{#if natW && (unit || angle || drawing)}
			<svg
				class="grid marks"
				viewBox="0 0 {natW} {natH}"
				preserveAspectRatio="xMidYMid meet"
				style={view}
				aria-hidden="true"
				data-testid="ref-marks"
			>
				{#each [['unit', unit], ['angle', angle], [lineMode, drawing]] as const as [kind, seg], i (i)}
					{#if seg}
						<line
							class={kind || 'unit'}
							x1={seg[0][0] * natW}
							y1={seg[0][1] * natH}
							x2={seg[1][0] * natW}
							y2={seg[1][1] * natH}
						/>
					{/if}
				{/each}
			</svg>
		{/if}
	</div>
	{#if unit || angle}
		<p class="marks-read" role="status">
			{#if unit}<span class="u">Unit marked: measure on the canvas with the Measure tool</span>{/if}
			{#if angle}<span class="a">Angle {Math.round(degOf(angle))}°: shown on the canvas</span>{/if}
		</p>
	{/if}
	<div class="tools" role="toolbar" aria-label="Reference view">
		<button
			type="button"
			aria-label="Zoom reference out"
			disabled={zoom <= 1}
			onclick={() => zoomBy(1 / 1.5)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Zoom reference in"
			disabled={zoom >= 8}
			onclick={() => zoomBy(1.5)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Flip reference"
			aria-pressed={flipped}
			onclick={() => (flipped = !flipped)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="M12 3v18M8 7 3 12l5 5V7ZM16 7l5 5-5 5V7Z" />
			</svg>
		</button>
		{#if onpick}
			<button
				type="button"
				aria-label="Pick a colour from the reference"
				title="Pick a colour from the picture (or Alt+click it)"
				aria-pressed={picking}
				onclick={() => (picking = !picking)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="m14 7 3 3M5 19l1.5-4.5L15 6a2.1 2.1 0 0 1 3 3l-8.5 8.5Z" />
				</svg>
			</button>
		{/if}
		{#if onunit}
			<button
				type="button"
				aria-label="Mark a unit on the reference"
				title="Measure: draw a line over a part (the head, say) to use as your unit"
				aria-pressed={lineMode === 'unit'}
				onclick={() => (lineMode = lineMode === 'unit' ? '' : 'unit')}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M3 17 17 3l4 4L7 21Z" /><path d="m7 13 2 2M10 10l2 2M13 7l2 2" />
				</svg>
			</button>
		{/if}
		{#if onangle}
			<button
				type="button"
				aria-label="Find an angle on the reference"
				title="Angle: draw a line along an edge; the canvas shows it to copy"
				aria-pressed={lineMode === 'angle'}
				onclick={() => (lineMode = lineMode === 'angle' ? '' : 'angle')}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M4 20h16M4 20 16 6" /><path d="M10 20a6 6 0 0 0-1.5-4" />
				</svg>
			</button>
		{/if}
		{#if unit || angle}
			<button
				type="button"
				class="text"
				onclick={() => {
					onunit?.(null);
					onangle?.(null);
				}}
			>
				Clear marks
			</button>
		{/if}
		{#if grid?.length}
			<button
				type="button"
				aria-label="Grid on the reference"
				title="The canvas grid, on the picture: copy one square at a time"
				aria-pressed={showGrid}
				onclick={() => (showGrid = !showGrid)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M4 4h16v16H4zM4 12h16M12 4v16" />
				</svg>
			</button>
		{/if}
		{#if zoom > 1 || flipped}
			<button type="button" class="text" onclick={reset}>Reset</button>
		{/if}
	</div>
	{#if pickNote}<p class="note" role="status">{pickNote}</p>{/if}
	<svg class="filters" aria-hidden="true">
		{#each [2, 3] as n (n)}
			<filter id="{fid}-{n}" color-interpolation-filters="sRGB">
				<feColorMatrix type="saturate" values="0" />
				<feComponentTransfer>
					<feFuncR type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
					<feFuncG type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
					<feFuncB type="discrete" tableValues={n === 2 ? '0 1' : '0 0.55 1'} />
				</feComponentTransfer>
			</filter>
		{/each}
	</svg>
</div>

<style>
	.ref {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
	}
	.ref.fill {
		flex: 1;
	}
	.box {
		position: relative;
		overflow: hidden;
		border-radius: 18px;
		background: var(--pp-surface-sunk);
		touch-action: pan-y;
	}
	.fill .box {
		flex: 1;
		min-height: 120px;
	}
	.box.zoomed {
		cursor: grab;
		touch-action: none;
	}
	.box.picking,
	.box.lining {
		cursor: crosshair;
		touch-action: none;
	}
	.marks line {
		stroke-width: 3;
		vector-effect: non-scaling-stroke;
		stroke-linecap: round;
	}
	.marks line.unit {
		stroke: #1f9d55;
	}
	.marks line.angle {
		stroke: #e8492f;
	}
	.marks-read {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 13px;
		font-weight: 600;
	}
	.marks-read .u {
		color: #1f9d55;
	}
	.marks-read .a {
		color: #e8492f;
	}
	.filters {
		position: absolute;
		width: 0;
		height: 0;
	}
	img {
		display: block;
		width: 100%;
		max-height: var(--ref-max-h, 320px);
		object-fit: contain;
		transition: transform 0.12s ease-out;
		user-select: none;
	}
	.fill img {
		height: 100%;
		max-height: none;
		position: absolute;
		inset: 0;
	}
	.grid {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
		transition: transform 0.12s ease-out;
	}
	.grid line {
		stroke: rgba(61, 107, 255, 0.75);
		stroke-width: 1.2;
		vector-effect: non-scaling-stroke;
	}
	/* the same pills as the canvas options bar (Pen pressure, QuickShape…) */
	.tools {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.tools button {
		display: grid;
		place-items: center;
		min-width: 38px;
		height: 38px;
		padding: 0 12px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tools button:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.tools button[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.note {
		margin: 0;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	svg {
		width: 18px;
		height: 18px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.grid {
		width: 100%;
		height: 100%;
		stroke-width: initial;
	}
	.fade-in {
		animation: fade 0.35s ease-out;
	}
	@keyframes fade {
		from {
			opacity: 0;
		}
	}
</style>
