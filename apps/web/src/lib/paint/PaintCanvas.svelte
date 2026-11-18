<script lang="ts" module>
	export interface CanvasStatus {
		canUndo: boolean;
		canRedo: boolean;
		blank: boolean;
		strokes: number;
	}

	export interface DrawingExport {
		width: number;
		height: number;
		/** PNG data URL of the drawing */
		png: string;
		/** grayscale bytes, width*height, for the grader */
		gray: Uint8Array;
		strokes: Stroke[];
	}
</script>

<script lang="ts">
	import { onMount } from 'svelte';
	import { loadBrush, loadEngine, PaintCanvas, type PaintBrush } from './brushes';
	import { StrokeLog, type Stroke } from './strokes';
	import { toGray } from '../grader';

	let {
		width = 800,
		height = 600,
		brushId,
		eraser = false,
		size = 1,
		opacity = 1,
		color = '#16161a',
		onchange
	}: {
		width?: number;
		height?: number;
		brushId: string;
		eraser?: boolean;
		/** multiplier on the brush's designed size */
		size?: number;
		opacity?: number;
		color?: string;
		onchange?: (s: CanvasStatus) => void;
	} = $props();

	let el: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;
	let memory: WebAssembly.Memory | undefined;
	let paint: PaintCanvas | undefined;
	let brush: PaintBrush | undefined;
	let ready = $state(false);
	let error = $state('');

	const log = new StrokeLog();
	const t0 = performance.now();
	let current: Stroke | null = null;
	let lastT = 0;
	let raf = 0;

	onMount(() => {
		let dead = false;
		loadEngine()
			.then((mem) => {
				if (dead) return;
				memory = mem;
				paint = new PaintCanvas(width, height);
				ctx = el.getContext('2d', { willReadFrequently: true });
				if (ctx) {
					ctx.fillStyle = '#fff';
					ctx.fillRect(0, 0, width, height);
				}
				ready = true;
				notify();
			})
			.catch((e) => (error = `Could not start the paint engine: ${e}`));
		return () => {
			dead = true;
			cancelAnimationFrame(raf);
			paint?.free();
			brush?.free();
		};
	});

	// Load the selected brush.
	$effect(() => {
		const id = brushId;
		if (!ready) return;
		let cancelled = false;
		loadBrush(id)
			.then((b) => {
				if (cancelled) return b.free();
				brush?.free();
				brush = b;
				apply();
			})
			.catch((e) => (error = `Could not load brush "${id}": ${e}`));
		return () => {
			cancelled = true;
		};
	});

	// Re-apply size / opacity / colour / eraser whenever they change.
	$effect(() => {
		apply();
	});

	function apply() {
		const [s, o, c, er] = [size, opacity, color, eraser]; // read props so the effect tracks them
		if (!brush) return;
		brush.setSizeScale(s);
		brush.setOpacity(o);
		const n = parseInt(c.slice(1), 16);
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		brush.setEraser(er);
	}

	function notify() {
		if (!paint) return;
		onchange?.({
			canUndo: paint.canUndo(),
			canRedo: paint.canRedo(),
			blank: paint.isBlank(),
			strokes: log.strokes.length
		});
	}

	function schedule() {
		if (!raf) raf = requestAnimationFrame(render);
	}

	function render() {
		raf = 0;
		if (!paint || !ctx || !memory) return;
		const r = paint.flush();
		if (r.length !== 4) return;
		const data = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), width * height * 4);
		ctx.putImageData(new ImageData(data, width, height), 0, 0, r[0], r[1], r[2], r[3]);
	}

	function feed(e: PointerEvent) {
		if (!paint || !brush || !current) return;
		const rect = el.getBoundingClientRect();
		const x = ((e.clientX - rect.left) * width) / rect.width;
		const y = ((e.clientY - rect.top) * height) / rect.height;
		const pressure = e.pointerType === 'mouse' ? 0.5 : e.pressure;
		const dt = Math.max(0, (e.timeStamp - lastT) / 1000);
		lastT = e.timeStamp;
		paint.strokeTo(brush, x, y, pressure, (e.tiltX || 0) / 90, (e.tiltY || 0) / 90, dt);
		current.points.push({
			x: Math.round(x * 10) / 10,
			y: Math.round(y * 10) / 10,
			pressure: Math.round(pressure * 1000) / 1000,
			t: Math.round(e.timeStamp - t0)
		});
	}

	function down(e: PointerEvent) {
		if (!paint || !brush || (e.pointerType === 'mouse' && e.button !== 0)) return;
		el.setPointerCapture(e.pointerId);
		paint.beginStroke();
		current = { brushId, eraser, points: [] };
		lastT = e.timeStamp;
		feed(e);
		schedule();
	}

	function move(e: PointerEvent) {
		if (!current) return;
		const events = e.getCoalescedEvents?.() ?? [];
		for (const ev of events.length ? events : [e]) feed(ev);
		schedule();
	}

	function up() {
		if (!current || !paint || !brush) return;
		paint.endStroke(brush);
		log.add(current);
		current = null;
		schedule();
		notify();
	}

	export function undo() {
		if (paint?.undo()) log.undo();
		schedule();
		notify();
	}

	export function redo() {
		if (paint?.redo()) log.redo();
		schedule();
		notify();
	}

	export function clear() {
		paint?.clear();
		log.clear();
		schedule();
		notify();
	}

	export function exportDrawing(): DrawingExport | null {
		if (!ctx) return null;
		render();
		return {
			width,
			height,
			png: el.toDataURL('image/png'),
			gray: toGray(ctx.getImageData(0, 0, width, height).data),
			strokes: structuredClone(log.strokes as Stroke[])
		};
	}
</script>

<div class="wrap" style="aspect-ratio: {width} / {height}">
	<canvas
		bind:this={el}
		{width}
		{height}
		aria-label="Drawing canvas, {width} by {height} pixels"
		data-ready={ready}
		onpointerdown={down}
		onpointermove={move}
		onpointerup={up}
		onpointercancel={up}
		onlostpointercapture={up}
	></canvas>
	{#if !ready && !error}<p class="status">Warming up the brushes…</p>{/if}
	{#if error}<p class="status error" role="alert">{error}</p>{/if}
</div>

<style>
	.wrap {
		position: relative;
		width: 100%;
	}
	canvas {
		display: block;
		width: 100%;
		height: 100%;
		touch-action: none;
		cursor: crosshair;
		border-radius: inherit;
		background: #fff;
	}
	.status {
		position: absolute;
		inset: 0;
		margin: 0;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
		font-weight: 600;
	}
	.error {
		color: var(--pp-accent-ink);
		padding: 24px;
		text-align: center;
	}
</style>
