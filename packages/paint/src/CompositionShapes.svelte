<svelte:options namespace="svg" />

<script lang="ts">
	/** A composition guide's shapes in an SVG: its solid lines, then its dashed ones. */
	import { arcsPath, type Shapes } from './composition-elements';

	let {
		parts,
		color = 'currentColor',
		opacity = 0.6,
		width = 1.5,
		dash = '8 6'
	}: {
		parts: { solid: Shapes; dashed: Shapes };
		/** any CSS colour (a var() too) */
		color?: string;
		opacity?: number;
		/** the lines' width in screen pixels */
		width?: number;
		dash?: string;
	} = $props();
</script>

{#each [parts.solid, parts.dashed] as s, k (k)}
	<g
		class={k ? 'dashed' : 'solid'}
		fill="none"
		style:stroke={color}
		style:stroke-opacity={opacity}
		style:stroke-width={width}
		style:stroke-dasharray={k ? dash : undefined}
	>
		{#each s.lines as [x1, y1, x2, y2], i (i)}
			<line {x1} {y1} {x2} {y2} vector-effect="non-scaling-stroke" />
		{/each}
		{#if s.arcs.length}
			<path
				class="spiral"
				d={arcsPath(s.arcs)}
				vector-effect="non-scaling-stroke"
				style:stroke-width={width + 0.5}
			/>
		{/if}
		{#each s.ellipses as e, i (i)}
			<ellipse cx={e.c[0]} cy={e.c[1]} rx={e.rx} ry={e.ry} vector-effect="non-scaling-stroke" />
		{/each}
	</g>
{/each}
