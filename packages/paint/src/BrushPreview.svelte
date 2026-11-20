<script lang="ts">
	/** A sample stroke drawn with a brush: an S curve from light to firm pressure and back. */
	import { brushFromSource, loadEngine, PaintCanvas } from './brushes';

	let {
		myb,
		color = '#16161a',
		label = 'Brush preview'
	}: { myb: string; color?: string; label?: string } = $props();

	const W = 320;
	const H = 96;
	let el: HTMLCanvasElement;

	async function draw(src: string, hex: string) {
		const memory = await loadEngine();
		const brush = await brushFromSource(src);
		const n = parseInt(hex.slice(1), 16) || 0;
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		const paint = new PaintCanvas(W, H);
		try {
			paint.beginStroke();
			const steps = 90;
			for (let i = 0; i <= steps; i++) {
				const t = i / steps;
				const x = 20 + t * (W - 40);
				const y = H / 2 + Math.sin(t * Math.PI * 2) * (H * 0.28);
				const pressure = 0.08 + 0.92 * Math.sin(t * Math.PI); // light → firm → light
				paint.strokeTo(brush, x, y, pressure, 0, 0, 0.012);
			}
			paint.endStroke(brush);
			paint.flush();
			const px = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), W * H * 4).slice();
			el?.getContext('2d')?.putImageData(new ImageData(px, W, H), 0, 0);
		} finally {
			paint.free();
			brush.free();
		}
	}

	let timer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const [src, hex] = [myb, color];
		clearTimeout(timer);
		timer = setTimeout(() => void draw(src, hex).catch(() => {}), 60);
		return () => clearTimeout(timer);
	});
</script>

<figure aria-label={label}><canvas bind:this={el} width={W} height={H}></canvas></figure>

<style>
	figure {
		margin: 0;
	}
	canvas {
		display: block;
		width: 100%;
		max-width: 320px;
		aspect-ratio: 320 / 96;
		border-radius: 12px;
		border: 1px solid var(--pp-track);
		background: #fff;
	}
</style>
