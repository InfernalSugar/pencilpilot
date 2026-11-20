<script lang="ts">
	import { resolve } from '$app/paths';
	/**
	 * S0-T05: what the pen gives us in this window. Draw on the pad; the panel shows
	 * the pointer type, pressure, tilt and twist, the barrel / eraser buttons, how many
	 * samples arrive per second (with coalesced ones), and how late events reach us.
	 * The trail's width follows pressure; its colour, tilt.
	 */
	import { onMount } from 'svelte';
	import { invoke, isTauri } from '@tauri-apps/api/core';

	let pad = $state<HTMLCanvasElement>();
	let ctx: CanvasRenderingContext2D | null = null;
	let info = $state({
		type: '—',
		pressure: 0,
		tiltX: 0,
		tiltY: 0,
		twist: 0,
		buttons: 0,
		width: 0,
		height: 0
	});
	let rate = $state(0);
	let coalescedRate = $state(0);
	let lagMs = $state(0);
	let maxPressureSeen = $state(0);
	let distinctPressures = $state(0);
	let hasTilt = $state(false);
	let hasCoalesced = $state(false);
	let hasPredicted = $state(false);
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a counter set, never rendered
	const pressures = new Set<number>();
	let samples: number[] = [];
	let coalesced: number[] = [];
	let last: { x: number; y: number } | null = null;
	/** where this runs: the app (and on which system) or a plain browser */
	let where = $state(isTauri() ? 'app' : 'browser');

	onMount(() => {
		if (isTauri())
			invoke<{ version: string; os: string; arch: string }>('app_info')
				.then((a) => (where = `app ${a.version}, ${a.os} ${a.arch}`))
				.catch(() => {});
		const resize = () => {
			if (!pad) return;
			const r = pad.getBoundingClientRect();
			pad.width = Math.round(r.width * devicePixelRatio);
			pad.height = Math.round(r.height * devicePixelRatio);
			ctx = pad.getContext('2d');
			ctx?.scale(devicePixelRatio, devicePixelRatio);
		};
		resize();
		window.addEventListener('resize', resize);
		const tick = setInterval(() => {
			const now = performance.now();
			samples = samples.filter((t) => now - t < 1000);
			coalesced = coalesced.filter((t) => now - t < 1000);
			rate = samples.length;
			coalescedRate = coalesced.length;
		}, 250);
		return () => {
			window.removeEventListener('resize', resize);
			clearInterval(tick);
		};
	});

	function point(e: PointerEvent) {
		const r = pad!.getBoundingClientRect();
		return { x: e.clientX - r.left, y: e.clientY - r.top };
	}

	function draw(e: PointerEvent) {
		if (!ctx) return;
		const p = point(e);
		if (last) {
			const hue = 200 + Math.max(-60, Math.min(60, e.tiltX)) * 2;
			ctx.strokeStyle = `hsl(${hue} 70% 40%)`;
			ctx.lineWidth = 1 + e.pressure * 14;
			ctx.lineCap = 'round';
			ctx.beginPath();
			ctx.moveTo(last.x, last.y);
			ctx.lineTo(p.x, p.y);
			ctx.stroke();
		}
		last = p;
	}

	function record(e: PointerEvent) {
		info = {
			type: e.pointerType,
			pressure: e.pressure,
			tiltX: e.tiltX,
			tiltY: e.tiltY,
			twist: e.twist,
			buttons: e.buttons,
			width: e.width,
			height: e.height
		};
		if (e.pressure > 0 && e.pressure !== 0.5) pressures.add(Math.round(e.pressure * 1000));
		distinctPressures = pressures.size;
		maxPressureSeen = Math.max(maxPressureSeen, e.pressure);
		if (e.tiltX || e.tiltY) hasTilt = true;
		// how long ago the event happened, by the time we handle it
		lagMs = Math.max(0, performance.now() - e.timeStamp);
	}

	function down(e: PointerEvent) {
		pad?.setPointerCapture(e.pointerId);
		last = point(e);
		record(e);
	}

	function move(e: PointerEvent) {
		record(e);
		samples.push(performance.now());
		if (e.buttons === 0) {
			last = null;
			return;
		}
		const list = e.getCoalescedEvents?.() ?? [];
		if (list.length) hasCoalesced = true;
		if (e.getPredictedEvents?.().length) hasPredicted = true;
		for (const c of list.length ? list : [e]) {
			coalesced.push(performance.now());
			draw(c);
		}
	}

	function up(e: PointerEvent) {
		record(e);
		last = null;
	}

	function clear() {
		if (pad && ctx) ctx.clearRect(0, 0, pad.width, pad.height);
		pressures.clear();
		distinctPressures = 0;
		maxPressureSeen = 0;
		hasTilt = false;
	}

	const verdict = $derived(
		info.type === 'pen'
			? distinctPressures > 20
				? 'Pressure works.'
				: 'A pen, but pressure looks flat. Press harder and softer while drawing.'
			: info.type === '—'
				? 'Draw on the pad with your pen.'
				: `That was a ${info.type}, not a pen.`
	);
</script>

<main class="test">
	<header>
		<a class="back" href={resolve('/')}>← Studio</a>
		<h1>Pen test</h1>
		<button type="button" onclick={clear}>Clear</button>
	</header>
	<div class="body">
		<canvas
			bind:this={pad}
			class="pad"
			data-testid="pen-pad"
			onpointerdown={down}
			onpointermove={move}
			onpointerup={up}
			onpointercancel={up}
		></canvas>
		<aside aria-label="Pen readings">
			<p class="verdict" role="status">{verdict}</p>
			<dl>
				<dt>Pointer</dt>
				<dd data-testid="pen-type">{info.type}</dd>
				<dt>Pressure</dt>
				<dd>{info.pressure.toFixed(3)} <small>(max {maxPressureSeen.toFixed(2)})</small></dd>
				<dt>Pressure levels seen</dt>
				<dd>{distinctPressures}</dd>
				<dt>Tilt</dt>
				<dd>{info.tiltX}°, {info.tiltY}° {hasTilt ? '' : '(none yet)'}</dd>
				<dt>Twist</dt>
				<dd>{info.twist}°</dd>
				<dt>Buttons</dt>
				<dd>
					{info.buttons}
					<small>(1 tip, 2 barrel, 32 eraser)</small>
				</dd>
				<dt>Contact size</dt>
				<dd>{info.width.toFixed(1)} × {info.height.toFixed(1)}</dd>
				<dt>Events / s</dt>
				<dd data-testid="pen-rate">{rate}</dd>
				<dt>Samples / s (coalesced)</dt>
				<dd>{coalescedRate} {hasCoalesced ? '' : '(none yet)'}</dd>
				<dt>Predicted events</dt>
				<dd>{hasPredicted ? 'yes' : 'no'}</dd>
				<dt>Event delay</dt>
				<dd>{lagMs.toFixed(1)} ms <small>(budget: under 16)</small></dd>
				<dt>Window</dt>
				<dd>
					{where}, {devicePixelRatio}× pixels
				</dd>
			</dl>
		</aside>
	</div>
</main>

<style>
	.test {
		display: flex;
		flex-direction: column;
		height: 100vh;
		padding: 12px;
		box-sizing: border-box;
		gap: 12px;
	}
	header {
		display: flex;
		align-items: center;
		gap: 16px;
	}
	h1 {
		margin: 0;
		font: 700 22px var(--pp-font-display);
	}
	header button,
	.back {
		min-height: 40px;
		padding: 0 14px;
		display: inline-grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
	}
	.body {
		flex: 1;
		min-height: 0;
		display: flex;
		gap: 12px;
	}
	.pad {
		flex: 1;
		min-width: 0;
		border-radius: 18px;
		background: #fff;
		touch-action: none;
		cursor: crosshair;
	}
	aside {
		width: 300px;
		padding: 16px;
		border-radius: 18px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		overflow: auto;
	}
	.verdict {
		margin: 0 0 12px;
		font-weight: 700;
	}
	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 6px 12px;
		margin: 0;
		font-size: 14px;
	}
	dt {
		color: var(--pp-on-ink-muted);
	}
	dd {
		margin: 0;
		font-variant-numeric: tabular-nums;
	}
	small {
		color: var(--pp-on-ink-muted);
	}
</style>
