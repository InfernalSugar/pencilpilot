<script lang="ts">
	/**
	 * A slider drawn as a bar that fills with its value: tall in the tool rail, wide when
	 * the rail lies across a narrow screen (`across`). Press or drag
	 * anywhere on it to set the value; the wheel and the arrow keys step it.
	 */
	let {
		label,
		value,
		min,
		max,
		step,
		text,
		across = false,
		onchange
	}: {
		label: string;
		value: number;
		min: number;
		max: number;
		step: number;
		/** the value as shown (and read out) */
		text: string;
		/** lying across (fills left to right) instead of standing (bottom to top) */
		across?: boolean;
		onchange: (v: number) => void;
	} = $props();

	const part = $derived(max > min ? Math.min(1, Math.max(0, (value - min) / (max - min))) : 0);

	function set(v: number) {
		const s = Math.round((v - min) / step) * step + min;
		const next = +Math.min(max, Math.max(min, s)).toFixed(6);
		if (next !== value) onchange(next);
	}

	function at(e: PointerEvent, el: HTMLElement) {
		const r = el.getBoundingClientRect();
		const t = across ? (e.clientX - r.left) / r.width : (r.bottom - e.clientY) / r.height;
		set(min + Math.min(1, Math.max(0, t)) * (max - min));
	}

	function down(e: PointerEvent) {
		if (e.button !== 0) return;
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		at(e, el);
	}

	function move(e: PointerEvent) {
		const el = e.currentTarget as HTMLElement;
		if (el.hasPointerCapture(e.pointerId)) at(e, el);
	}

	function key(e: KeyboardEvent) {
		const big = (max - min) / 10;
		const by: Record<string, number> = {
			ArrowUp: step,
			ArrowRight: step,
			ArrowDown: -step,
			ArrowLeft: -step,
			PageUp: big,
			PageDown: -big
		};
		if (e.key === 'Home') set(min);
		else if (e.key === 'End') set(max);
		else if (e.key in by) set(value + by[e.key]);
		else return;
		e.preventDefault();
	}

	let bar = $state<HTMLElement>();
	// (the wheel steps it, not the page: a listener that may prevent scrolling)
	$effect(() => {
		const el = bar;
		if (!el) return;
		const wheel = (e: WheelEvent) => {
			e.preventDefault();
			set(value + (e.deltaY < 0 ? step : -step));
		};
		el.addEventListener('wheel', wheel, { passive: false });
		return () => el.removeEventListener('wheel', wheel);
	});
</script>

<div class="fill-slider" class:across>
	<span class="label">{label} <b>{text}</b></span>
	<div
		class="bar"
		role="slider"
		tabindex="0"
		aria-label={label}
		aria-orientation={across ? 'horizontal' : 'vertical'}
		aria-valuemin={min}
		aria-valuemax={max}
		aria-valuenow={value}
		aria-valuetext={text}
		onpointerdown={down}
		onpointermove={move}
		onkeydown={key}
		bind:this={bar}
		style:--part={part}
	>
		<div class="fill"></div>
	</div>
</div>

<style>
	.fill-slider {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		width: 44px;
		text-align: center;
		font-size: 11px;
		color: var(--pp-on-ink-muted);
	}
	.label b {
		display: block;
		color: var(--pp-on-ink);
	}
	.bar {
		position: relative;
		width: 30px;
		height: 140px;
		border-radius: 10px;
		background: #2a2a30;
		box-shadow: inset 0 0 0 1px #4a4a54;
		overflow: hidden;
		cursor: ns-resize;
		touch-action: none;
	}
	.bar:focus-visible {
		outline: 2px solid var(--pp-yellow);
		outline-offset: 2px;
	}
	.fill {
		position: absolute;
		left: 0;
		right: 0;
		bottom: 0;
		height: calc(var(--part) * 100%);
		min-height: 3px;
		background: var(--pp-yellow);
		opacity: 0.85;
	}
	/* (the edge of the fill: a line to read the value against) */
	.fill::after {
		content: '';
		position: absolute;
		left: 4px;
		right: 4px;
		top: 0;
		height: 2px;
		border-radius: 1px;
		background: #fff;
	}
	.across {
		flex-direction: row;
		width: auto;
		text-align: left;
	}
	.across .label b {
		display: inline;
	}
	.across .bar {
		flex: 1 1 auto;
		cursor: ew-resize;
	}
	.across .fill {
		top: 0;
		right: auto;
		height: auto;
		width: calc(var(--part) * 100%);
		min-width: 3px;
	}
	.across .fill::after {
		left: auto;
		right: 0;
		top: 4px;
		bottom: 4px;
		width: 2px;
		height: auto;
	}
</style>
