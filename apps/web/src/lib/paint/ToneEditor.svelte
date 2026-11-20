<script lang="ts">
	/**
	 * Curves, Levels and Gradient Map editors for the Adjust panel. Changes go
	 * into `spec` and `onchange` runs so the canvas can preview them live.
	 */
	import {
		cleanCurve,
		curveTable,
		MAP_PRESETS,
		NO_CURVES,
		NO_LEVELS,
		type AdjustSpec,
		type CurvePt,
		type Curves
	} from './tone';

	let {
		kind,
		spec = $bindable(),
		onchange
	}: {
		kind: 'curves' | 'levels' | 'map';
		spec: AdjustSpec;
		onchange: () => void;
	} = $props();

	// ---- curves ----

	type Channel = keyof Curves;
	let channel = $state<Channel>('rgb');
	const curves = $derived(spec.curves ?? NO_CURVES);
	const pts = $derived(curves[channel]);
	const SIZE = 200;
	const table = $derived(curveTable(pts));
	const line = $derived(
		Array.from({ length: 65 }, (_, i) => {
			const x = Math.round((i / 64) * 255);
			return `${(x / 255) * SIZE},${SIZE - (table[x] / 255) * SIZE}`;
		}).join(' ')
	);
	const COLOURS: Record<Channel, string> = {
		rgb: 'currentColor',
		r: '#e5484d',
		g: '#30a46c',
		b: '#3e63dd'
	};
	let svg = $state<SVGSVGElement>();
	let drag: { id: number; index: number } | null = null;

	function setCurve(next: CurvePt[]) {
		spec = { ...spec, curves: { ...curves, [channel]: next } };
		onchange();
	}

	/** room around the graph, so the end points can be grabbed */
	const PAD = 12;
	function at(e: PointerEvent): CurvePt {
		const r = svg!.getBoundingClientRect();
		const full = SIZE + 2 * PAD;
		return [
			((e.clientX - r.left) / r.width) * (full / SIZE) - PAD / SIZE,
			1 - (((e.clientY - r.top) / r.height) * (full / SIZE) - PAD / SIZE)
		];
	}

	function down(e: PointerEvent) {
		const p = at(e);
		const near = pts.findIndex((q) => Math.hypot(q[0] - p[0], q[1] - p[1]) < 0.06);
		(e.currentTarget as Element).setPointerCapture(e.pointerId);
		if (near >= 0) {
			drag = { id: e.pointerId, index: near };
			return;
		}
		const next = cleanCurve([...pts, p]);
		setCurve(next);
		drag = {
			id: e.pointerId,
			index: next.findIndex((q) => q[0] === Math.min(1, Math.max(0, p[0])))
		};
	}

	function move(e: PointerEvent) {
		if (!drag || drag.id !== e.pointerId) return;
		const p = at(e);
		const i = drag.index;
		// a point stays between its neighbours (so the curve never folds back)
		const lo = i === 0 ? 0 : pts[i - 1][0] + 0.01;
		const hi = i === pts.length - 1 ? 1 : pts[i + 1][0] - 0.01;
		const x = Math.min(hi, Math.max(lo, p[0]));
		setCurve(pts.map((q, k) => (k === i ? ([x, Math.min(1, Math.max(0, p[1]))] as CurvePt) : q)));
	}

	function up(e: PointerEvent) {
		if (!drag || drag.id !== e.pointerId) return;
		// dragged far off the graph: the point goes (never the last two)
		const p = at(e);
		if (pts.length > 2 && (p[1] < -0.12 || p[1] > 1.12 || p[0] < -0.12 || p[0] > 1.12))
			setCurve(pts.filter((_, k) => k !== drag!.index));
		drag = null;
	}

	// ---- levels ----

	const levels = $derived(spec.levels ?? NO_LEVELS);
	function setLevel(k: keyof typeof NO_LEVELS, v: number) {
		spec = { ...spec, levels: { ...levels, [k]: v } };
		onchange();
	}

	// ---- gradient map ----

	const stops = $derived(spec.map ?? MAP_PRESETS[0].stops);
	function setStops(next: typeof stops) {
		spec = { ...spec, map: next };
		onchange();
	}
	const mapCss = $derived(
		`linear-gradient(90deg, ${[...stops]
			.sort((a, b) => a.at - b.at)
			.map((s) => `${s.color} ${Math.round(s.at * 100)}%`)
			.join(', ')})`
	);
</script>

{#if kind === 'curves'}
	<div class="curves" data-testid="curves">
		<div class="seg" role="radiogroup" aria-label="Curve channel">
			{#each [['rgb', 'RGB'], ['r', 'Red'], ['g', 'Green'], ['b', 'Blue']] as [id, label] (id)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={channel === id}
					onclick={() => (channel = id as Channel)}
				>
					{label}
				</button>
			{/each}
		</div>
		<svg
			bind:this={svg}
			class="graph"
			viewBox="{-PAD} {-PAD} {SIZE + 2 * PAD} {SIZE + 2 * PAD}"
			role="application"
			aria-label="Curve: click to add a point, drag to bend, drag a point off the graph to remove it"
			onpointerdown={down}
			onpointermove={move}
			onpointerup={up}
			onpointercancel={up}
		>
			<rect x="0" y="0" width={SIZE} height={SIZE} class="frame" />
			{#each [0.25, 0.5, 0.75] as g (g)}
				<line x1={g * SIZE} y1="0" x2={g * SIZE} y2={SIZE} class="grid" />
				<line x1="0" y1={g * SIZE} x2={SIZE} y2={g * SIZE} class="grid" />
			{/each}
			<line x1="0" y1={SIZE} x2={SIZE} y2="0" class="grid diag" />
			<polyline points={line} fill="none" stroke={COLOURS[channel]} stroke-width="2.5" />
			{#each pts as [x, y], i (i)}
				<circle cx={x * SIZE} cy={SIZE - y * SIZE} r="6" class="pt" />
			{/each}
		</svg>
		<button
			type="button"
			class="tool"
			onclick={() => {
				spec = { ...spec, curves: null };
				onchange();
			}}
		>
			Reset curves
		</button>
	</div>
{:else if kind === 'levels'}
	<div class="levels" data-testid="levels">
		{#each [['inBlack', 'Black point', 0, 0.95], ['gamma', 'Midtones', 0.2, 5], ['inWhite', 'White point', 0.05, 1], ['outBlack', 'Darkest', 0, 1], ['outWhite', 'Lightest', 0, 1]] as [k, label, min, max] (k)}
			{@const key = k as keyof typeof NO_LEVELS}
			<label class="slider">
				<span>
					{label}
					<b>{key === 'gamma' ? levels[key].toFixed(2) : Math.round(levels[key] * 255)}</b>
				</span>
				<input
					type="range"
					min={min as number}
					max={max as number}
					step="0.01"
					value={levels[key]}
					oninput={(e) => setLevel(key, +e.currentTarget.value)}
				/>
			</label>
		{/each}
	</div>
{:else}
	<div class="map" data-testid="gradient-map">
		<div class="bar" style="background: {mapCss}" aria-hidden="true"></div>
		<div class="presets" role="group" aria-label="Gradient presets">
			{#each MAP_PRESETS as p (p.id)}
				<button
					type="button"
					class="preset"
					title={p.name}
					aria-label={p.name}
					style="background: linear-gradient(90deg, {p.stops
						.map((s) => `${s.color} ${s.at * 100}%`)
						.join(', ')})"
					onclick={() => setStops(p.stops)}
				></button>
			{/each}
		</div>
		<div class="stops">
			{#each stops as s, i (i)}
				<label class="stop">
					<input
						type="color"
						value={s.color}
						aria-label={i === 0
							? 'Darkest colour'
							: i === stops.length - 1
								? 'Lightest colour'
								: 'Middle colour'}
						oninput={(e) =>
							setStops(stops.map((x, k) => (k === i ? { ...x, color: e.currentTarget.value } : x)))}
					/>
				</label>
			{/each}
		</div>
	</div>
{/if}

<style>
	.curves,
	.levels,
	.map {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
	}
	/* the options bar's buttons (this sits inside it) */
	.seg {
		display: flex;
		gap: 2px;
	}
	.tool {
		min-width: 38px;
		height: 38px;
		padding: 0 12px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink, #fff);
		font: 600 14px var(--pp-font-body, sans-serif);
		cursor: pointer;
	}
	.tool[aria-checked='true'] {
		background: var(--pp-accent-strong, #2f6bff);
		color: #fff;
	}
	.slider {
		display: flex;
		flex-direction: column;
		gap: 4px;
		width: 150px;
		font-size: 13px;
		color: var(--pp-on-ink-muted, #c8c8d0);
	}
	.slider b {
		color: var(--pp-on-ink, #fff);
	}
	.graph {
		width: 224px;
		height: 224px;
		border-radius: 10px;
		background: var(--pp-surface, #fff);
		color: var(--pp-ink, #16161a);
		touch-action: none;
		cursor: crosshair;
	}
	.frame {
		fill: none;
		stroke: currentColor;
		stroke-opacity: 0.3;
	}
	.grid {
		stroke: currentColor;
		stroke-opacity: 0.15;
	}
	.diag {
		stroke-dasharray: 4 4;
	}
	.pt {
		fill: #fff;
		stroke: #2f6bff;
		stroke-width: 2.5;
	}
	.bar {
		width: 220px;
		height: 22px;
		border-radius: 8px;
	}
	.presets,
	.stops {
		display: flex;
		gap: 6px;
	}
	.preset {
		width: 40px;
		height: 40px;
		border: 2px solid rgba(255, 255, 255, 0.4);
		border-radius: 10px;
		cursor: pointer;
	}
	.stop input {
		width: 40px;
		height: 40px;
		padding: 0;
		border: 0;
		background: none;
		cursor: pointer;
	}
</style>
