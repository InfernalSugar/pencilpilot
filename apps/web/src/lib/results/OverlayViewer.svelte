<script lang="ts" module>
	export type OverlayMode = 'fade' | 'heatmap' | 'size' | 'points' | 'side';

	type Pt = { x: number; y: number };
	/** The proportions check: per tapped point, fractions of the canvas. */
	export type PointsView = { label: string; ref: Pt; mine: Pt; error: number }[];
</script>

<script lang="ts">
	import type { Grade } from '../grader';

	let {
		referenceSrc,
		drawingSrc,
		grade,
		mode = $bindable('fade'),
		highlight = null,
		aspect = 4 / 3,
		points = null
	}: {
		referenceSrc: string;
		drawingSrc: string;
		grade: Grade | null;
		mode?: OverlayMode;
		/** a fix's overlay hint to emphasise */
		highlight?: string | null;
		/** the canvas's width ÷ height, so boxes and the heatmap line up */
		aspect?: number;
		/** the proportions check, when the student did it */
		points?: PointsView | null;
	} = $props();

	/** 0 = only your drawing, 1 = only the reference */
	let mix = $state(0.35);

	const MODES = $derived<{ id: OverlayMode; label: string }[]>([
		{ id: 'fade', label: 'Overlay' },
		{ id: 'heatmap', label: 'Heatmap' },
		{ id: 'size', label: 'Size & position' },
		...(points?.length ? [{ id: 'points' as const, label: 'Proportions' }] : []),
		{ id: 'side', label: 'Side by side' }
	]);
	/** the point that's furthest off (only called out when it's clearly off) */
	const worstPoint = $derived(
		points?.length ? points.reduce((w, p, i) => (p.error > points[w].error ? i : w), 0) : -1
	);
	const chain = (ps: Pt[]) => ps.map((p) => `${p.x * 100},${(p.y * 100) / aspect}`).join(' ');

	const cells = $derived(
		grade
			? grade.grid.cells.map((v, i) => ({
					v,
					col: i % grade.grid.cols,
					row: Math.floor(i / grade.grid.cols)
				}))
			: []
	);
	const worst = $derived(grade?.grid.worst ?? null);
	const pctBox = (b: { x: number; y: number; w: number; h: number }) =>
		`left:${b.x * 100}%;top:${b.y * 100}%;width:${b.w * 100}%;height:${b.h * 100}%`;
	/** red (bad) → yellow → transparent (good) */
	const heat = (v: number) =>
		v >= 0.8
			? 'transparent'
			: v >= 0.6
				? 'rgba(255, 216, 77, 0.35)'
				: v >= 0.4
					? 'rgba(242, 169, 59, 0.45)'
					: 'rgba(232, 73, 47, 0.5)';
</script>

<div class="viewer">
	<div class="modes" role="tablist" aria-label="Compare view">
		{#each MODES as m (m.id)}
			<button
				type="button"
				role="tab"
				aria-selected={mode === m.id}
				disabled={!grade && (m.id === 'heatmap' || m.id === 'size')}
				onclick={() => (mode = m.id)}
			>
				{m.label}
			</button>
		{/each}
	</div>

	{#if mode === 'side'}
		<div class="side">
			<figure>
				<div class="frame" style="aspect-ratio: {aspect}; width: min(100%, calc(75vh * {aspect}))">
					<img src={referenceSrc} alt="The reference" />
				</div>
				<figcaption>Reference</figcaption>
			</figure>
			<figure>
				<div class="frame" style="aspect-ratio: {aspect}; width: min(100%, calc(75vh * {aspect}))">
					<img src={drawingSrc} alt="Your drawing" />
				</div>
				<figcaption>Your drawing</figcaption>
			</figure>
		</div>
	{:else}
		<div
			class="frame stack"
			data-testid="overlay"
			style="aspect-ratio: {aspect}; width: min(100%, calc(75vh * {aspect}))"
		>
			<img
				src={referenceSrc}
				alt="The reference"
				style="opacity: {mode === 'fade'
					? mix
					: mode === 'size'
						? 0.25
						: mode === 'points'
							? 0.45
							: 0.15}"
			/>
			<!-- the proportions view lines your points up with the reference, so your lines would mislead -->
			<img
				class="drawing"
				src={drawingSrc}
				alt="Your drawing"
				style="opacity: {mode === 'fade' ? 1 - mix * 0.6 : mode === 'points' ? 0 : 1}"
			/>

			{#if points?.length && mode === 'points'}
				<svg
					class="arrow"
					viewBox="0 0 100 {100 / aspect}"
					preserveAspectRatio="none"
					aria-hidden="true"
				>
					<polyline class="chain ref" points={chain(points.map((p) => p.ref))} />
					<polyline class="chain mine" points={chain(points.map((p) => p.mine))} />
					{#each points as p, i (i)}
						<line
							class="gap"
							class:worst={i === worstPoint && p.error > 0.08}
							x1={p.mine.x * 100}
							y1={(p.mine.y * 100) / aspect}
							x2={p.ref.x * 100}
							y2={(p.ref.y * 100) / aspect}
						/>
					{/each}
				</svg>
				{#each points as p, i (i)}
					<span
						class="kp ref"
						style="left:{p.ref.x * 100}%;top:{p.ref.y * 100}%"
						title="{p.label} (reference)"
					></span>
					<span
						class="kp mine"
						class:worst={i === worstPoint && p.error > 0.08}
						style="left:{p.mine.x * 100}%;top:{p.mine.y * 100}%"
						title="{p.label} (yours): {Math.round(p.error * 100)}% off"
					>
						{#if i === worstPoint && p.error > 0.08}<em>{p.label}</em>{/if}
					</span>
				{/each}
			{/if}

			{#if grade && mode === 'heatmap'}
				{#each cells as c (c.col + ',' + c.row)}
					<span
						class="cell"
						class:worst={worst && worst.col === c.col && worst.row === c.row && c.v < 0.75}
						style="left:{(c.col / grade.grid.cols) * 100}%;top:{(c.row / grade.grid.rows) *
							100}%;width:{100 / grade.grid.cols}%;height:{100 / grade.grid.rows}%;background:{heat(
							c.v
						)}"
						title="{Math.round(c.v * 100)}% match"
					></span>
				{/each}
			{/if}

			{#if grade && mode === 'size'}
				{#if grade.bbox.reference}
					<span class="box ref" style={pctBox(grade.bbox.reference)}><em>Reference</em></span>
				{/if}
				{#if grade.bbox.drawing}
					<span class="box mine" style={pctBox(grade.bbox.drawing)}><em>Yours</em></span>
				{/if}
				{#if grade.bbox.reference && grade.bbox.drawing}
					{@const r = grade.bbox.reference}
					{@const d = grade.bbox.drawing}
					<svg
						class="arrow"
						viewBox="0 0 100 {100 / aspect}"
						preserveAspectRatio="none"
						aria-hidden="true"
					>
						<defs>
							<marker id="pp-head" markerWidth="6" markerHeight="6" refX="5" refY="3" orient="auto">
								<path d="M0,0 L6,3 L0,6 Z" fill="var(--pp-accent)" />
							</marker>
						</defs>
						<line
							x1={(d.x + d.w / 2) * 100}
							y1={(d.y + d.h / 2) * 75}
							x2={(r.x + r.w / 2) * 100}
							y2={(r.y + r.h / 2) * 75}
							stroke="var(--pp-accent)"
							stroke-width="0.8"
							marker-end="url(#pp-head)"
						/>
					</svg>
				{/if}
			{/if}
		</div>
		{#if mode === 'fade'}
			<label class="slider">
				<span>Your drawing</span>
				<input
					type="range"
					min="0"
					max="1"
					step="0.01"
					bind:value={mix}
					aria-label="Fade between your drawing and the reference"
				/>
				<span>Reference</span>
			</label>
		{:else if mode === 'heatmap'}
			<p class="legend">
				<span class="sw" style="background: rgba(232, 73, 47, 0.5)"></span>far off
				<span class="sw" style="background: rgba(242, 169, 59, 0.45)"></span>some way off
				<span class="sw" style="background: rgba(255, 216, 77, 0.35)"></span>close
				<span class="sw" style="background: #fff; border: 1px solid var(--pp-track)"></span>match
			</p>
		{:else if mode === 'points'}
			<p class="legend">
				<span class="sw ring"></span>reference <span class="sw dot"></span>your points, lined up
				with the reference (size, position and tilt don’t count here, only proportions)
			</p>
		{:else if mode === 'size'}
			<p class="legend">
				<span class="sw dash"></span>reference outline <span class="sw solid"></span>your drawing
				<span class="sw arrow-sw"></span>move this way
			</p>
		{/if}
	{/if}
	{#if highlight && mode !== 'side'}
		<span class="sr-only" role="status">Showing: {highlight}</span>
	{/if}
</div>

<style>
	.viewer {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.modes {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		padding: 4px;
		background: var(--pp-ground);
		border-radius: 999px;
		align-self: flex-start;
	}
	.modes button {
		padding: 8px 14px;
		border: 0;
		border-radius: 999px;
		background: transparent;
		font: 600 13px var(--pp-font-body);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.modes button[aria-selected='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.modes button:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.frame {
		position: relative;
		margin-inline: auto;
		background: #fff;
		border-radius: 20px;
		overflow: hidden;
		border: 1px solid var(--pp-track);
	}
	.frame img {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		transition: opacity 0.15s;
	}
	.stack .drawing {
		mix-blend-mode: multiply;
	}
	.chain {
		fill: none;
		stroke-width: 0.5;
		vector-effect: non-scaling-stroke;
	}
	.chain.ref {
		stroke: var(--pp-ink);
		stroke-dasharray: 4 3;
		stroke-width: 1.5;
	}
	.chain.mine {
		stroke: var(--pp-accent);
		stroke-width: 2;
	}
	.gap {
		stroke: var(--pp-accent);
		stroke-width: 1;
		vector-effect: non-scaling-stroke;
		opacity: 0.6;
	}
	.gap.worst {
		stroke-width: 2.5;
		opacity: 1;
	}
	.kp {
		position: absolute;
		width: 14px;
		height: 14px;
		margin: -7px 0 0 -7px;
		border-radius: 50%;
	}
	.kp.ref {
		border: 2px solid var(--pp-ink);
		background: #fff;
	}
	.kp.mine {
		background: var(--pp-accent);
		border: 2px solid #fff;
	}
	.kp.mine.worst {
		width: 20px;
		height: 20px;
		margin: -10px 0 0 -10px;
		box-shadow: 0 0 0 3px var(--pp-accent);
	}
	.kp em {
		position: absolute;
		left: 22px;
		top: -4px;
		white-space: nowrap;
		font: 600 12px var(--pp-font-body);
		font-style: normal;
		background: var(--pp-accent);
		color: #fff;
		padding: 2px 6px;
		border-radius: 999px;
	}
	.sw.ring {
		border: 2px solid var(--pp-ink);
		border-radius: 50%;
		background: #fff;
	}
	.sw.dot {
		border-radius: 50%;
		background: var(--pp-accent);
	}
	.side {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}
	figure {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	figcaption,
	.legend,
	.slider {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-muted);
	}
	.cell {
		position: absolute;
		border: 1px solid rgba(255, 255, 255, 0.6);
	}
	.cell.worst {
		border: 3px solid var(--pp-accent);
		border-radius: 8px;
	}
	.box {
		position: absolute;
		border-radius: 6px;
	}
	.box em {
		position: absolute;
		top: -22px;
		left: 0;
		font: 600 11px var(--pp-font-body);
		font-style: normal;
		padding: 2px 6px;
		border-radius: 6px;
		background: #fff;
	}
	.box.ref {
		border: 3px dashed var(--pp-cat-real);
	}
	.box.ref em {
		color: var(--pp-cat-real);
	}
	.box.mine {
		border: 3px solid var(--pp-accent);
	}
	.box.mine em {
		color: var(--pp-accent-ink);
		top: auto;
		bottom: -22px;
	}
	.arrow {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	.slider {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.slider input {
		flex: 1;
		accent-color: var(--pp-accent);
	}
	.legend {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 10px;
	}
	.sw {
		display: inline-block;
		width: 14px;
		height: 14px;
		border-radius: 4px;
	}
	.sw.dash {
		border: 2px dashed var(--pp-cat-real);
	}
	.sw.solid {
		border: 2px solid var(--pp-accent);
	}
	.sw.arrow-sw {
		height: 3px;
		background: var(--pp-accent);
	}
	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
