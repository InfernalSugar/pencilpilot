<script lang="ts">
	/**
	 * Rulers along the top and left of the view, in pixels, centimetres or inches, following
	 * the zoom and pan (and a mirrored view). A turned view has none: its sides no longer
	 * line up with the canvas's.
	 */
	import { rulerTicks, type Unit } from './view-prefs';

	let {
		width,
		height,
		viewW,
		viewH,
		originX,
		originY,
		scaleX,
		scaleY,
		unit,
		ppi
	}: {
		/** the view's size (screen px) */
		viewW: number;
		viewH: number;
		/** the canvas's size (canvas px) */
		width: number;
		height: number;
		/** where canvas pixel 0 is, from the view's top left (screen px) */
		originX: number;
		originY: number;
		/** screen px per canvas px (negative: mirrored) */
		scaleX: number;
		scaleY: number;
		unit: Unit;
		ppi: number;
	} = $props();

	const T = 20;
	const top = $derived(rulerTicks(originX - T, scaleX, viewW - T, unit, ppi));
	const left = $derived(rulerTicks(originY - T, scaleY, viewH - T, unit, ppi));
	/** the canvas's own span on each ruler (shaded lighter) */
	const spanX = $derived([originX, originX + width * scaleX].sort((a, b) => a - b));
	const spanY = $derived([originY, originY + height * scaleY].sort((a, b) => a - b));
</script>

<svg class="rulers" width={viewW} height={viewH} aria-hidden="true" data-testid="rulers">
	<rect class="bar" x={T} y="0" width={viewW - T} height={T} />
	<rect class="bar" x="0" y={T} width={T} height={viewH - T} />
	<rect
		class="span"
		x={Math.max(T, spanX[0])}
		y="0"
		width={Math.max(0, spanX[1] - Math.max(T, spanX[0]))}
		height={T}
	/>
	<rect
		class="span"
		x="0"
		y={Math.max(T, spanY[0])}
		width={T}
		height={Math.max(0, spanY[1] - Math.max(T, spanY[0]))}
	/>
	{#each top as t, i (i)}
		<line x1={T + t.at} x2={T + t.at} y1={t.label ? 4 : 14} y2={T} />
		{#if t.label}<text x={T + t.at + 3} y="10">{t.label}</text>{/if}
	{/each}
	{#each left as t, i (i)}
		<line y1={T + t.at} y2={T + t.at} x1={t.label ? 4 : 14} x2={T} />
		{#if t.label}
			<text transform="translate(10 {T + t.at + 3}) rotate(-90)" text-anchor="end">{t.label}</text>
		{/if}
	{/each}
	<rect class="corner" x="0" y="0" width={T} height={T} />
	<text class="unit" x={T / 2} y="13" text-anchor="middle">{unit}</text>
</svg>

<style>
	.rulers {
		position: absolute;
		inset: 0;
		z-index: 4;
		pointer-events: none;
		font: 600 9px var(--pp-font-body, sans-serif);
	}
	.bar,
	.corner {
		fill: var(--pp-surface, #f4f2ec);
		opacity: 0.94;
	}
	.span {
		fill: var(--pp-ground, #fff);
	}
	line {
		stroke: var(--pp-muted, #6b6b73);
		stroke-width: 1;
		shape-rendering: crispEdges;
	}
	text {
		fill: var(--pp-ink, #16161a);
	}
	.unit {
		fill: var(--pp-muted, #6b6b73);
	}
</style>
