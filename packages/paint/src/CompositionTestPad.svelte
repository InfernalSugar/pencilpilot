<script lang="ts">
	/**
	 * A little canvas under the guide, to sketch on and see how the guide works in use.
	 * Plain pencil lines (pen pressure counts); nothing here is kept.
	 */
	import CompositionShapes from './CompositionShapes.svelte';
	import { defShapes, type CompElement } from './composition-elements';
	import { applyCurve } from './pressure';

	let {
		elements,
		color,
		opacity,
		width,
		height,
		ink = '#16161a'
	}: {
		elements: CompElement[];
		color: string;
		opacity: number;
		width: number;
		height: number;
		ink?: string;
	} = $props();

	const parts = $derived(defShapes({ elements }, width, height));
	let el = $state<HTMLCanvasElement>();
	let stroke: { id: number; last: [number, number] } | null = null;
	/** something has been drawn (Clear shows then) */
	let inked = $state(false);

	function at(e: PointerEvent): [number, number] {
		const r = el!.getBoundingClientRect();
		return [((e.clientX - r.left) / r.width) * width, ((e.clientY - r.top) / r.height) * height];
	}

	function down(e: PointerEvent) {
		if (stroke || e.button > 0) return;
		e.preventDefault();
		el?.setPointerCapture?.(e.pointerId);
		stroke = { id: e.pointerId, last: at(e) };
		inked = true;
	}
	function move(e: PointerEvent) {
		if (stroke?.id !== e.pointerId) return;
		const g = el?.getContext('2d');
		if (!g) return;
		g.strokeStyle = ink;
		g.lineCap = 'round';
		for (const c of e.getCoalescedEvents?.() ?? [e]) {
			const p = at(c);
			const pressure = c.pointerType === 'pen' ? applyCurve(c.pressure) : 0.5;
			g.lineWidth = 0.6 + pressure * 2.4;
			g.beginPath();
			g.moveTo(...stroke.last);
			g.lineTo(...p);
			g.stroke();
			stroke.last = p;
		}
	}
	const up = (e: PointerEvent) => {
		if (stroke?.id === e.pointerId) stroke = null;
	};

	function clear() {
		el?.getContext('2d')?.clearRect(0, 0, width, height);
		inked = false;
	}
</script>

<div class="pad" style:max-width="{width}px">
	<canvas
		bind:this={el}
		{width}
		{height}
		style:aspect-ratio="{width} / {height}"
		aria-label="Sketch here to try the guide"
		data-testid="composition-test-pad"
		onpointerdown={down}
		onpointermove={move}
		onpointerup={up}
		onpointercancel={up}
	></canvas>
	<svg viewBox="0 0 {width} {height}" aria-hidden="true">
		<CompositionShapes {parts} {color} {opacity} />
	</svg>
	{#if !inked}<span class="hint" aria-hidden="true">Sketch here to try it</span>{/if}
	<button type="button" class="clear" onclick={clear} disabled={!inked}>Clear</button>
</div>

<style>
	.pad {
		position: relative;
		margin: 0 auto;
		width: 100%;
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
	svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
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
