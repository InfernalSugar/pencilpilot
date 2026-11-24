<script lang="ts">
	/**
	 * A composition guide over a sample picture, redrawn as it changes. The ends of the
	 * line picked in the creator can be dragged here.
	 */
	import CompositionShapes from './CompositionShapes.svelte';
	import { defShapes, type CompElement, type Pt } from './composition-elements';

	let {
		elements,
		color,
		opacity,
		width,
		height,
		selected = -1,
		onpoint
	}: {
		elements: CompElement[];
		color: string;
		opacity: number;
		/** the frame's size, px */
		width: number;
		height: number;
		/** the element picked in the list (a line's ends can be dragged) */
		selected?: number;
		/** a line's end dragged to `p` (0–1 of the frame) */
		onpoint?: (i: number, end: 'a' | 'b', p: Pt) => void;
	} = $props();

	const parts = $derived(defShapes({ elements }, width, height));
	const line = $derived.by(() => {
		const e = elements[selected];
		return e?.kind === 'line' ? e : null;
	});

	let svg = $state<SVGSVGElement>();
	let drag: { end: 'a' | 'b'; id: number } | null = null;
	function at(e: PointerEvent): Pt {
		const r = svg!.getBoundingClientRect();
		const k = (v: number) => Math.round(Math.max(0, Math.min(1, v)) * 1000) / 1000;
		return [k((e.clientX - r.left) / r.width), k((e.clientY - r.top) / r.height)];
	}
</script>

<svg
	bind:this={svg}
	class="preview"
	viewBox="0 0 {width} {height}"
	style:aspect-ratio="{width} / {height}"
	style:max-width="{width}px"
	role="img"
	aria-label="Preview of the guide"
	data-testid="composition-preview"
	onpointermove={(e) => {
		if (drag?.id !== e.pointerId || !onpoint) return;
		onpoint(selected, drag.end, at(e));
	}}
	onpointerup={() => (drag = null)}
	onpointercancel={() => (drag = null)}
>
	<!-- a sample picture: sky, sun, hills -->
	<rect class="sky" {width} {height} />
	<circle class="sun" cx={width * 0.7} cy={height * 0.3} r={Math.min(width, height) * 0.09} />
	<path
		class="hill"
		d="M0 {height * 0.7} C{width * 0.25} {height * 0.55} {width * 0.45} {height * 0.62} {width *
			0.6} {height * 0.72} S{width * 0.9} {height * 0.66} {width} {height * 0.6} V{height} H0 Z"
	/>
	<CompositionShapes {parts} {color} {opacity} />
	{#if line && onpoint}
		{#each ['a', 'b'] as const as end (end)}
			<circle
				class="end"
				data-testid="line-end-{end}"
				cx={line[end][0] * width}
				cy={line[end][1] * height}
				r="7"
				role="presentation"
				onpointerdown={(e) => {
					e.preventDefault();
					svg?.setPointerCapture?.(e.pointerId);
					drag = { end, id: e.pointerId };
				}}
			/>
		{/each}
	{/if}
</svg>

<style>
	.preview {
		display: block;
		width: 100%;
		margin: 0 auto;
		border-radius: 12px;
		border: 1px solid var(--pp-track);
		touch-action: none;
	}
	.sky {
		fill: #fff;
	}
	.sun {
		fill: #f1f1f4;
		stroke: #dcdce2;
	}
	.hill {
		fill: #ececf0;
	}
	.end {
		fill: #fff;
		stroke: #16161a;
		stroke-width: 2;
		cursor: grab;
	}
</style>
