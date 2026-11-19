<script lang="ts" module>
	export type BlendMode =
		'normal' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten' | 'add';

	export interface LayerInfo {
		id: number;
		name: string;
		visible: boolean;
		/** 0–1 */
		opacity: number;
		blend: BlendMode;
		locked: boolean;
		alphaLock: boolean;
		empty: boolean;
	}
	export type LayerSettings = Omit<LayerInfo, 'id' | 'empty'>;

	export interface CanvasStatus {
		canUndo: boolean;
		canRedo: boolean;
		blank: boolean;
		strokes: number;
		/** bottom → top */
		layers: LayerInfo[];
		active: number;
		/** bumps on every change (for refreshing layer thumbnails) */
		version: number;
	}

	/** Status before the engine has loaded. */
	export const BLANK_STATUS: CanvasStatus = {
		canUndo: false,
		canRedo: false,
		blank: true,
		strokes: 0,
		layers: [],
		active: 0,
		version: 0
	};

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
		onchange,
		onblocked,
		overlaySrc = null,
		overlayOpacity = 0.3,
		overlayTransform = { x: 0, y: 0, scale: 1, rotate: 0 },
		adjustOverlay = false,
		onoverlaymove,
		locked = false,
		maxHeight = '78vh',
		viewControls = true
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
		/** a stroke was refused because the active layer is locked or hidden */
		onblocked?: (why: 'locked' | 'hidden') => void;
		/** a picture shown over the canvas (fitted like the grader fits references); never part of the drawing */
		overlaySrc?: string | null;
		overlayOpacity?: number;
		/** where the overlay sits: offset (fraction of the canvas), scale and rotation (degrees) */
		overlayTransform?: { x: number; y: number; scale: number; rotate: number };
		/** no more strokes (e.g. time is up); a stroke in progress is finished */
		locked?: boolean;
		/** dragging moves the overlay instead of drawing */
		adjustOverlay?: boolean;
		/** overlay dragged by (dx, dy), as fractions of the canvas width / height */
		onoverlaymove?: (dx: number, dy: number) => void;
		/** tallest the canvas area may get */
		maxHeight?: string;
		/** show the zoom / rotate / mirror bar */
		viewControls?: boolean;
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

	let version = 0;

	function layerList(): LayerInfo[] {
		return paint ? (JSON.parse(paint.layersJson()) as LayerInfo[]) : [];
	}

	function visibleLayerIds(): Set<number> {
		return new Set(
			layerList()
				.filter((l) => l.visible && l.opacity > 0)
				.map((l) => l.id)
		);
	}

	function notify() {
		if (!paint) return;
		const layers = layerList();
		onchange?.({
			canUndo: paint.canUndo(),
			canRedo: paint.canRedo(),
			blank: paint.isBlank(),
			strokes: log.on(new Set(layers.filter((l) => l.visible).map((l) => l.id))).length,
			layers,
			active: paint.activeLayer,
			version: ++version
		});
	}

	/** run an engine change, keep the history in step, then redraw */
	function changed(recorded: boolean, logIt: () => void = () => log.layerChange()) {
		if (recorded) logIt();
		schedule();
		notify();
		return recorded;
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

	// ---- view: zoom, pan, rotate, mirror (only how it's shown, never the drawing) ----

	let vp: HTMLDivElement;
	let vpW = $state(0);
	let vpH = $state(0);
	let zoom = $state(1);
	let rot = $state(0);
	let flipped = $state(false);
	let panX = $state(0);
	let panY = $state(0);
	const ZOOM_MIN = 0.25;
	const ZOOM_MAX = 16;
	/** CSS px per canvas px at zoom 1 (the canvas fitted inside the viewport) */
	const fit = $derived(vpW && vpH ? Math.min(vpW / width, vpH / height) : 0);
	const baseW = $derived(width * fit);
	const baseH = $derived(height * fit);
	const sheetTransform = $derived(
		`translate(-50%, -50%) translate(${panX}px, ${panY}px) rotate(${rot}deg) scale(${zoom * (flipped ? -1 : 1)}, ${zoom})`
	);

	$effect(() => {
		const ro = new ResizeObserver(([e]) => {
			vpW = e.contentRect.width;
			vpH = e.contentRect.height;
		});
		ro.observe(vp);
		return () => ro.disconnect();
	});

	/** client (screen) position → canvas pixels, undoing pan, rotation, zoom and mirror */
	function toCanvas(clientX: number, clientY: number): [number, number] {
		const r = vp.getBoundingClientRect();
		const dx = clientX - (r.left + r.width / 2) - panX;
		const dy = clientY - (r.top + r.height / 2) - panY;
		const a = (-rot * Math.PI) / 180;
		const ux = dx * Math.cos(a) - dy * Math.sin(a);
		const uy = dx * Math.sin(a) + dy * Math.cos(a);
		const lx = ux / (zoom * (flipped ? -1 : 1));
		const ly = uy / zoom;
		return [(lx + baseW / 2) / fit, (ly + baseH / 2) / fit];
	}

	function clampPan() {
		const mx = vpW / 2 + (baseW * zoom) / 2 - 48;
		const my = vpH / 2 + (baseH * zoom) / 2 - 48;
		panX = Math.max(-mx, Math.min(mx, panX));
		panY = Math.max(-my, Math.min(my, panY));
	}

	/** zoom by `f`, keeping the point under (clientX, clientY) still (default: the centre) */
	function zoomAt(f: number, clientX?: number, clientY?: number) {
		const z = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, zoom * f));
		const k = z / zoom;
		if (clientX !== undefined && clientY !== undefined) {
			const r = vp.getBoundingClientRect();
			const sx = clientX - (r.left + r.width / 2);
			const sy = clientY - (r.top + r.height / 2);
			panX = sx - k * (sx - panX);
			panY = sy - k * (sy - panY);
		} else {
			panX *= k;
			panY *= k;
		}
		zoom = z;
		clampPan();
	}

	export function zoomIn() {
		zoomAt(1.25);
	}
	export function zoomOut() {
		zoomAt(1 / 1.25);
	}
	/** back to the whole canvas, upright */
	export function resetView() {
		zoom = 1;
		rot = 0;
		panX = 0;
		panY = 0;
		flipped = false;
	}
	export function rotateBy(deg: number) {
		const a = (deg * Math.PI) / 180;
		[panX, panY] = [
			panX * Math.cos(a) - panY * Math.sin(a),
			panX * Math.sin(a) + panY * Math.cos(a)
		];
		rot = ((((rot + deg) % 360) + 540) % 360) - 180;
	}
	export function resetRotation() {
		rotateBy(-rot);
	}
	/** mirror the view (a classic way to spot proportion mistakes) */
	export function toggleMirror() {
		flipped = !flipped;
		panX = -panX;
	}

	// ---- input ----

	/** touch points currently down (for pinch / rotate / pan) */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- input bookkeeping, never rendered
	const touches = new Map<number, { x: number; y: number }>();
	let gesture: {
		mx: number;
		my: number;
		d: number;
		a: number;
		zoom: number;
		rot: number;
		panX: number;
		panY: number;
	} | null = null;
	/** after a pinch, remaining fingers do nothing until all are lifted */
	let gestureLatch = false;
	let panning: { id: number; x: number; y: number; panX: number; panY: number } | null = null;
	let moving: { id: number; cx: number; cy: number } | null = null;
	let spaceHeld = $state(false);
	let hovering = false;
	/** once a pen has been used, fingers only move the view (palm rejection) */
	let penSeen = false;

	function twoTouches() {
		const [p, q] = [...touches.values()];
		return {
			mx: (p.x + q.x) / 2,
			my: (p.y + q.y) / 2,
			d: Math.hypot(q.x - p.x, q.y - p.y) || 1,
			a: Math.atan2(q.y - p.y, q.x - p.x)
		};
	}

	function feed(e: PointerEvent) {
		if (!paint || !brush || !current) return;
		const [x, y] = toCanvas(e.clientX, e.clientY);
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

	function startPan(e: PointerEvent) {
		vp.setPointerCapture(e.pointerId);
		panning = { id: e.pointerId, x: e.clientX, y: e.clientY, panX, panY };
	}

	function down(e: PointerEvent) {
		if (!paint || !brush) return;
		// the view toolbar sits on top of the canvas area: its buttons aren't strokes
		if ((e.target as HTMLElement).closest('.view')) return;
		if (e.pointerType === 'pen') penSeen = true;
		if (e.pointerType === 'touch') {
			touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
			if (touches.size === 2) {
				// a second finger: this is a pinch, not a stroke
				if (current) {
					paint.cancelStroke();
					current = null;
					schedule();
				}
				panning = null;
				const g = twoTouches();
				gesture = { ...g, zoom, rot, panX, panY };
				gestureLatch = true;
				return;
			}
			if (touches.size > 2 || gestureLatch) return;
			if (penSeen) return startPan(e);
		}
		if (e.pointerType === 'mouse' && e.button === 1) return startPan(e);
		if (spaceHeld) return startPan(e);
		if (e.pointerType === 'mouse' && e.button !== 0) return;
		if (locked) return;
		if (adjustOverlay && overlaySrc) {
			vp.setPointerCapture(e.pointerId);
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			moving = { id: e.pointerId, cx, cy };
			return;
		}
		if (!paint.beginStroke()) {
			const l = layerList().find((x) => x.id === paint!.activeLayer);
			onblocked?.(l && !l.visible ? 'hidden' : 'locked');
			return;
		}
		vp.setPointerCapture(e.pointerId);
		current = { brushId, eraser, layer: paint.activeLayer, points: [] };
		lastT = e.timeStamp;
		feed(e);
		schedule();
	}

	function move(e: PointerEvent) {
		if (e.pointerType === 'touch' && touches.has(e.pointerId)) {
			touches.set(e.pointerId, { x: e.clientX, y: e.clientY });
			if (gesture && touches.size >= 2) {
				const g = twoTouches();
				const r = vp.getBoundingClientRect();
				const [cx, cy] = [r.left + r.width / 2, r.top + r.height / 2];
				zoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, (gesture.zoom * g.d) / gesture.d));
				const da = g.a - gesture.a;
				rot = gesture.rot + (da * 180) / Math.PI;
				// keep the canvas point under the first midpoint under the current one
				const f = zoom / gesture.zoom;
				const [ox, oy] = [gesture.mx - cx - gesture.panX, gesture.my - cy - gesture.panY];
				panX = g.mx - cx - f * (ox * Math.cos(da) - oy * Math.sin(da));
				panY = g.my - cy - f * (ox * Math.sin(da) + oy * Math.cos(da));
				return;
			}
		}
		if (moving && e.pointerId === moving.id) {
			const [cx, cy] = toCanvas(e.clientX, e.clientY);
			onoverlaymove?.((cx - moving.cx) / width, (cy - moving.cy) / height);
			moving = { ...moving, cx, cy };
			return;
		}
		if (panning && e.pointerId === panning.id) {
			panX = panning.panX + e.clientX - panning.x;
			panY = panning.panY + e.clientY - panning.y;
			clampPan();
			return;
		}
		if (!current) return;
		const events = e.getCoalescedEvents?.() ?? [];
		for (const ev of events.length ? events : [e]) feed(ev);
		schedule();
	}

	function up(e: PointerEvent) {
		if (e.pointerType === 'touch') {
			touches.delete(e.pointerId);
			if (touches.size < 2) gesture = null;
			if (touches.size === 0) gestureLatch = false;
		}
		if (panning && e.pointerId === panning.id) panning = null;
		if (moving && e.pointerId === moving.id) moving = null;
		if (!current || !paint || !brush) return;
		paint.endStroke(brush);
		log.add(current);
		current = null;
		schedule();
		notify();
	}

	// Time's up mid-stroke: keep what was drawn so far.
	$effect(() => {
		if (locked && current && paint && brush) {
			paint.endStroke(brush);
			log.add(current);
			current = null;
			schedule();
			notify();
		}
	});

	// Ctrl/⌘ + wheel (and trackpad pinch) zooms at the pointer; the plain wheel pans once zoomed in.
	$effect(() => {
		const onWheel = (e: WheelEvent) => {
			if (e.ctrlKey || e.metaKey) {
				e.preventDefault();
				zoomAt(Math.exp(-e.deltaY * 0.01), e.clientX, e.clientY);
			} else if (zoom > 1.01) {
				e.preventDefault();
				panX -= e.deltaX;
				panY -= e.deltaY;
				clampPan();
			}
		};
		vp.addEventListener('wheel', onWheel, { passive: false });
		return () => vp.removeEventListener('wheel', onWheel);
	});

	function typing(t: EventTarget | null) {
		return t instanceof HTMLElement && !!t.closest('input, textarea, select, [contenteditable]');
	}
	function keydown(e: KeyboardEvent) {
		if (e.code === 'Space' && hovering && !typing(e.target)) {
			e.preventDefault();
			spaceHeld = true;
		}
	}
	function keyup(e: KeyboardEvent) {
		if (e.code === 'Space') spaceHeld = false;
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

	/** Erase the active layer. */
	export function clear() {
		if (!paint) return;
		const layer = paint.activeLayer;
		changed(paint.clear(), () => log.clear(layer));
	}

	// ---- layers ----

	export function selectLayer(id: number) {
		if (paint?.setActiveLayer(id)) notify();
	}

	export function addLayer(name = '') {
		if (paint) changed(paint.addLayer(name) !== 0);
	}

	export function duplicateLayer(id: number) {
		if (paint) changed(paint.duplicateLayer(id) !== 0);
	}

	export function deleteLayer(id: number) {
		if (paint) changed(paint.deleteLayer(id));
	}

	/** `to`: 0 = bottom */
	export function moveLayer(id: number, to: number) {
		if (paint) changed(paint.moveLayer(id, to));
	}

	export function mergeDown(id: number) {
		if (!paint) return;
		const layers = layerList();
		const i = layers.findIndex((l) => l.id === id);
		const below = layers[i - 1]?.id;
		if (below === undefined) return;
		changed(paint.mergeDown(id), () => log.merge(id, below));
	}

	function settings(id: number): LayerSettings | null {
		const l = layerList().find((x) => x.id === id);
		if (!l) return null;
		const { name, visible, opacity, blend, locked, alphaLock } = l;
		return { name, visible, opacity, blend, locked, alphaLock };
	}

	function setRaw(id: number, s: LayerSettings, record: boolean) {
		return paint!.setLayer(
			id,
			s.name,
			s.visible,
			s.opacity,
			s.blend,
			s.locked,
			s.alphaLock,
			record
		);
	}

	/** Change layer settings as one undo step. */
	export function setLayer(id: number, patch: Partial<LayerSettings>) {
		const cur = paint && settings(id);
		if (!cur) return;
		changed(setRaw(id, { ...cur, ...patch }, true));
	}

	let previewing: { id: number; before: LayerSettings } | null = null;

	/** Live preview (e.g. while dragging the opacity slider); finish with `commitLayer`. */
	export function previewLayer(id: number, patch: Partial<LayerSettings>) {
		const cur = paint && settings(id);
		if (!cur) return;
		if (previewing?.id !== id) previewing = { id, before: cur };
		setRaw(id, { ...cur, ...patch }, false);
		schedule();
		notify();
	}

	export function commitLayer() {
		if (!paint || !previewing) return;
		const { id, before } = previewing;
		previewing = null;
		const b = before;
		changed(paint.commitLayer(id, b.name, b.visible, b.opacity, b.blend, b.locked, b.alphaLock));
	}

	/** Straight-alpha preview of one layer. */
	export function layerThumbnail(id: number, w: number, h: number): ImageData | null {
		if (!paint) return null;
		return new ImageData(new Uint8ClampedArray(paint.layerThumbnail(id, w, h)), w, h);
	}

	/**
	 * The drawing as shown, minus the `exclude` layers (e.g. sketch-mode
	 * construction lines), with the strokes on the layers that remain.
	 */
	export function exportDrawing(opts: { exclude?: number[] } = {}): DrawingExport | null {
		if (!ctx || !paint) return null;
		const hide = (opts.exclude ?? []).filter((id) => layerList().some((l) => l.id === id));
		const restore: [number, LayerSettings][] = [];
		for (const id of hide) {
			const s = settings(id)!;
			restore.push([id, s]);
			setRaw(id, { ...s, visible: false }, false);
		}
		render();
		const ids = visibleLayerIds();
		const out = {
			width,
			height,
			png: el.toDataURL('image/png'),
			gray: toGray(ctx.getImageData(0, 0, width, height).data),
			strokes: structuredClone(log.on(ids))
		};
		// put the hidden layers back (in the same task, so nothing flickers)
		for (const [id, s] of restore) setRaw(id, s, false);
		if (restore.length) render();
		return out;
	}

	/** Forget undo/redo (after setting up starter layers). */
	export function clearHistory() {
		paint?.clearHistory();
		log.forgetHistory();
		notify();
	}
</script>

<svelte:window onkeydown={keydown} onkeyup={keyup} onblur={() => (spaceHeld = false)} />

<div
	class="viewport"
	class:grab={spaceHeld}
	class:moving-overlay={adjustOverlay && !!overlaySrc}
	bind:this={vp}
	style="aspect-ratio: {width} / {height}; max-height: {maxHeight}"
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	onlostpointercapture={up}
	onpointerenter={() => (hovering = true)}
	onpointerleave={() => (hovering = false)}
	role="presentation"
>
	<div class="sheet" style="width: {baseW}px; height: {baseH}px; transform: {sheetTransform}">
		<canvas
			bind:this={el}
			{width}
			{height}
			aria-label="Drawing canvas, {width} by {height} pixels"
			data-ready={ready}
		></canvas>
		{#if overlaySrc}
			<img
				class="overlay"
				src={overlaySrc}
				alt=""
				style="opacity: {overlayOpacity}; transform: translate({overlayTransform.x *
					100}%, {overlayTransform.y *
					100}%) rotate({overlayTransform.rotate}deg) scale({overlayTransform.scale})"
			/>
		{/if}
	</div>
	{#if viewControls && ready}
		<div class="view" role="toolbar" aria-label="View">
			<button type="button" aria-label="Zoom out" title="Zoom out (−)" onclick={zoomOut}>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
			</button>
			<button
				type="button"
				class="pct"
				title="Fit the whole canvas (0)"
				aria-label="Zoom {Math.round(zoom * 100)}%, reset view"
				onclick={resetView}
			>
				{Math.round(zoom * 100)}%
			</button>
			<button type="button" aria-label="Zoom in" title="Zoom in (+)" onclick={zoomIn}>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
			</button>
			<button
				type="button"
				aria-label="Rotate left"
				title="Rotate left (4)"
				onclick={() => rotateBy(-15)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M4 4v6h6" /><path d="M4.5 15a8 8 0 1 0 1.9-8.3L4 10" />
				</svg>
			</button>
			<button
				type="button"
				aria-label="Rotate right"
				title="Rotate right (6)"
				onclick={() => rotateBy(15)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M20 4v6h-6" /><path d="M19.5 15a8 8 0 1 1-1.9-8.3L20 10" />
				</svg>
			</button>
			<button
				type="button"
				aria-label="Mirror view"
				aria-pressed={flipped}
				title="Mirror the view (M): fresh eyes for proportions"
				onclick={toggleMirror}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M12 3v18M8 7 3 12l5 5V7ZM16 7l5 5-5 5V7Z" />
				</svg>
			</button>
			{#if rot !== 0}
				<span class="angle" aria-live="polite">{Math.round(rot)}°</span>
			{/if}
		</div>
	{/if}
	{#if !ready && !error}<p class="status">Warming up the brushes…</p>{/if}
	{#if error}<p class="status error" role="alert">{error}</p>{/if}
</div>

<style>
	.viewport {
		position: relative;
		width: 100%;
		overflow: hidden;
		touch-action: none;
		cursor: crosshair;
		border-radius: inherit;
		background: var(--pp-surface-sunk);
		user-select: none;
		-webkit-user-select: none;
	}
	.viewport.grab {
		cursor: grab;
	}
	.viewport.moving-overlay {
		cursor: move;
	}
	.sheet {
		overflow: hidden;
	}
	.sheet {
		position: absolute;
		left: 50%;
		top: 50%;
		background: #fff;
		box-shadow: 0 1px 6px rgba(22, 22, 26, 0.12);
	}
	canvas {
		display: block;
		width: 100%;
		height: 100%;
		touch-action: none;
		background: #fff;
	}
	.overlay {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		pointer-events: none;
		mix-blend-mode: multiply;
	}
	.view {
		position: absolute;
		top: 10px;
		right: 10px;
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 4px;
		border-radius: 999px;
		background: color-mix(in srgb, var(--pp-surface) 92%, transparent);
		box-shadow: 0 1px 4px rgba(22, 22, 26, 0.15);
	}
	.view button {
		display: grid;
		place-items: center;
		min-width: 32px;
		height: 32px;
		padding: 0 6px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		color: var(--pp-ink);
		font: 600 12px var(--pp-font-body);
		cursor: pointer;
	}
	.view button[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.view svg {
		width: 16px;
		height: 16px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.angle {
		padding: 0 8px 0 4px;
		font: 600 12px var(--pp-font-body);
		color: var(--pp-ink-2);
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
