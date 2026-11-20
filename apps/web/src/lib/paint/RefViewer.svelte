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
		onpick
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
	} = $props();

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

	const view = $derived(
		`transform: translate(${panX}px, ${panY}px) scale(${zoom * (flipped ? -1 : 1)}, ${zoom})`
	);
</script>

<div class="ref" class:fill>
	<div
		class="box"
		class:zoomed={zoom > 1}
		class:picking
		bind:this={box}
		role="presentation"
		data-testid="ref-box"
		ondblclick={() => {
			if (picking) return;
			if (zoom > 1) reset();
			else zoomBy(2);
		}}
		onpointerdown={(e) => {
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
			if (!drag || e.pointerId !== drag.id) return;
			panX = drag.px + e.clientX - drag.x;
			panY = drag.py + e.clientY - drag.y;
		}}
		onpointerup={() => (drag = null)}
		onpointercancel={() => (drag = null)}
	>
		<img
			class="fade-in"
			class:gray
			bind:this={img}
			{src}
			{alt}
			draggable="false"
			style={view}
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
	</div>
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
	.box.picking {
		cursor: crosshair;
	}
	img {
		display: block;
		width: 100%;
		max-height: var(--ref-max-h, 320px);
		object-fit: contain;
		transition: transform 0.12s ease-out;
		user-select: none;
	}
	img.gray {
		filter: grayscale(1);
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
