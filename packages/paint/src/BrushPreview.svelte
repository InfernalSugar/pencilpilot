<script lang="ts">
	/**
	 * A sample stroke drawn with a brush: an S curve from light to firm pressure and
	 * back. It draws as the canvas does (designed size, full opacity, its texture).
	 */
	import { brushFromSource, loadEngine, PaintCanvas } from './brushes';
	import { styleStrokes, type BrushTexture } from './brush-textures';

	let {
		myb,
		texture = null,
		color = '#16161a',
		label = 'Brush preview',
		width = 320,
		height = 96,
		fit = false
	}: {
		myb: string;
		texture?: BrushTexture | null;
		color?: string;
		label?: string;
		width?: number;
		height?: number;
		/** shrink a big brush to fit (a small picture of it) */
		fit?: boolean;
	} = $props();

	let el = $state<HTMLCanvasElement>();
	/** bumped after each drawing (tests wait for it) */
	let drawn = $state(0);

	async function draw(
		src: string,
		tex: BrushTexture | null,
		hex: string,
		W: number,
		H: number,
		small: boolean
	) {
		const memory = await loadEngine();
		const brush = await brushFromSource(src);
		const n = parseInt(hex.slice(1), 16) || 0;
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		brush.setSizeScale(1);
		if (small) brush.setSizeScale(Math.min(1, (H * 0.12) / brush.radius()));
		brush.setOpacity(1);
		const paint = new PaintCanvas(W, H);
		try {
			styleStrokes(paint, tex, new Map());
			paint.beginStroke();
			const steps = Math.round(W / 3.5);
			// keep big brushes inside the picture
			const r = Math.min(H * 0.3, brush.radius() * 1.5);
			const amp = Math.max(4, H / 2 - r - 6);
			const m = Math.min(20, W * 0.1) + r;
			for (let i = 0; i <= steps; i++) {
				const t = i / steps;
				const x = m + t * (W - 2 * m);
				const y = H / 2 + Math.sin(t * Math.PI * 2) * amp * 0.75;
				const pressure = 0.08 + 0.92 * Math.sin(t * Math.PI); // light → firm → light
				paint.strokeTo(brush, x, y, pressure, 0, 0, 0.012);
			}
			paint.endStroke(brush);
			paint.flush();
			const px = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), W * H * 4).slice();
			el?.getContext('2d')?.putImageData(new ImageData(px, W, H), 0, 0);
			drawn++;
		} finally {
			paint.free();
			brush.free();
		}
	}

	let timer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const args = [
			myb,
			texture ? $state.snapshot(texture) : null,
			color,
			width,
			height,
			fit
		] as const;
		clearTimeout(timer);
		timer = setTimeout(() => void draw(...args).catch(() => {}), 60);
		return () => clearTimeout(timer);
	});
</script>

<figure aria-label={label || undefined} aria-hidden={!label || undefined} data-drawn={drawn}>
	<canvas bind:this={el} {width} {height} style:aspect-ratio="{width} / {height}"></canvas>
</figure>

<style>
	figure {
		margin: 0;
	}
	canvas {
		display: block;
		width: 100%;
		border-radius: 12px;
		border: 1px solid var(--pp-track);
		background: #fff;
	}
</style>
