<script lang="ts">
	/**
	 * A little canvas to try a brush on before keeping it. It paints with the same
	 * engine as the real canvas (pen pressure and tilt, the brush's texture), and
	 * the next stroke always uses the brush as it is set now.
	 */
	import { onDestroy, onMount } from 'svelte';
	import { brushFromSource, loadEngine, PaintCanvas, type PaintBrush } from './brushes';
	import { styleStrokes, type BrushTexture } from './brush-textures';
	import { applyCurve } from './pressure';

	let {
		myb,
		texture = null,
		color = '#16161a',
		width = 520,
		height = 220
	}: {
		myb: string;
		texture?: BrushTexture | null;
		color?: string;
		width?: number;
		height?: number;
	} = $props();

	let el = $state<HTMLCanvasElement>();
	let memory: WebAssembly.Memory | null = null;
	let paint: PaintCanvas | null = null;
	let brush: PaintBrush | null = null;
	/** pictures this canvas has been given (app id → engine id) */
	let texIds = new Map<string, number>();
	let stroke: { id: number; t: number } | null = null;
	/** something has been drawn (Clear shows then) */
	let inked = $state(false);
	let gone = false;

	onMount(async () => {
		memory = await loadEngine();
		if (gone) return;
		paint = new PaintCanvas(width, height);
		styleStrokes(paint, texture, texIds);
	});
	onDestroy(() => {
		gone = true;
		paint?.free();
		brush?.free();
		paint = brush = null;
	});

	// the brush as set now
	let loading = 0;
	$effect(() => {
		const src = myb;
		const ticket = ++loading;
		void brushFromSource(src)
			.then((b) => {
				if (gone || ticket !== loading) return b.free();
				brush?.free();
				brush = b;
				dress();
			})
			.catch(() => {});
	});
	$effect(() => {
		void color;
		dress();
	});
	$effect(() => {
		const t = texture ? $state.snapshot(texture) : null;
		if (paint && !stroke) styleStrokes(paint, t as BrushTexture | null, texIds);
	});

	function dress() {
		const hex = color;
		if (!brush) return;
		const n = parseInt(hex.slice(1), 16) || 0;
		brush.setColor((n >> 16) & 255, (n >> 8) & 255, n & 255);
		brush.setSizeScale(1);
		brush.setOpacity(1);
	}

	function at(e: PointerEvent): [number, number] {
		const r = el!.getBoundingClientRect();
		return [((e.clientX - r.left) / r.width) * width, ((e.clientY - r.top) / r.height) * height];
	}

	function point(e: PointerEvent) {
		if (!paint || !brush || !stroke) return;
		const [x, y] = at(e);
		const pen = e.pointerType === 'pen';
		const p = pen ? applyCurve(e.pressure) : e.pointerType === 'touch' ? e.pressure || 0.5 : 0.5;
		const dt = Math.max(0, (e.timeStamp - stroke.t) / 1000);
		stroke.t = e.timeStamp;
		paint.strokeTo(
			brush,
			x,
			y,
			p,
			pen ? (e.tiltX || 0) / 90 : 0,
			pen ? (e.tiltY || 0) / 90 : 0,
			dt
		);
	}

	let raf = 0;
	function show() {
		raf ||= requestAnimationFrame(() => {
			raf = 0;
			if (!paint || !memory || !el) return;
			const r = paint.flush();
			if (r.length !== 4) return;
			const data = new Uint8ClampedArray(memory.buffer, paint.rgbaPtr(), width * height * 4);
			el.getContext('2d')?.putImageData(
				new ImageData(data, width, height),
				0,
				0,
				r[0],
				r[1],
				r[2],
				r[3]
			);
		});
	}

	function down(e: PointerEvent) {
		if (!paint || !brush || stroke || e.button > 0) return;
		e.preventDefault();
		el?.setPointerCapture?.(e.pointerId);
		if (!paint.beginStroke()) return;
		stroke = { id: e.pointerId, t: e.timeStamp };
		point(e);
		inked = true;
		show();
	}

	function move(e: PointerEvent) {
		if (stroke?.id !== e.pointerId) return;
		for (const c of e.getCoalescedEvents?.() ?? [e]) point(c);
		show();
	}

	function up(e: PointerEvent) {
		if (stroke?.id !== e.pointerId || !paint || !brush) return;
		paint.endStroke(brush);
		stroke = null;
		styleStrokes(paint, texture ? ($state.snapshot(texture) as BrushTexture) : null, texIds);
		show();
	}

	/** a clean sheet */
	export function clear() {
		if (!memory || gone) return;
		paint?.free();
		paint = new PaintCanvas(width, height);
		texIds = new Map();
		styleStrokes(paint, texture ? ($state.snapshot(texture) as BrushTexture) : null, texIds);
		stroke = null;
		inked = false;
		el?.getContext('2d')?.clearRect(0, 0, width, height);
	}
</script>

<div class="pad">
	<canvas
		bind:this={el}
		{width}
		{height}
		style:aspect-ratio="{width} / {height}"
		aria-label="Try the brush here"
		onpointerdown={down}
		onpointermove={move}
		onpointerup={up}
		onpointercancel={up}
	></canvas>
	{#if !inked}<span class="hint" aria-hidden="true">Draw here to try it</span>{/if}
	<button type="button" class="clear" onclick={clear} disabled={!inked}>Clear</button>
</div>

<style>
	.pad {
		position: relative;
	}
	canvas {
		display: block;
		width: 100%;
		border-radius: 12px;
		border: 1px solid var(--pp-track);
		background: #fff;
		touch-action: none;
		cursor: crosshair;
	}
	.hint {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		color: #8a8a94;
		font: 600 14px var(--pp-font-body);
		pointer-events: none;
	}
	.clear {
		position: absolute;
		top: 8px;
		right: 8px;
		min-height: 32px;
		padding: 0 12px;
		border: 1px solid #d8d8de;
		border-radius: 10px;
		background: #fff;
		color: #16161a;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.clear:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
