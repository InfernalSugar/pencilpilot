<script lang="ts">
	/** Pen feel: how hard you have to press. Shared by every drawing screen. */
	import {
		PRESSURE_PRESETS,
		applyCurve,
		onPressureFirmness,
		pressureFirmness,
		setPressureFirmness
	} from './pressure';

	let firmness = $state(pressureFirmness());
	$effect(() => onPressureFirmness((f) => (firmness = f)));

	const path = $derived(
		Array.from({ length: 21 }, (_, i) => {
			const x = i / 20;
			return `${i ? 'L' : 'M'}${(x * 60).toFixed(1)},${(60 - applyCurve(x, firmness) * 60).toFixed(1)}`;
		}).join('')
	);
	const near = (f: number) => Math.abs(f - firmness) < 0.05;
</script>

<section class="pen" aria-label="Pen feel">
	<svg viewBox="-2 -2 64 64" width="56" height="56" aria-hidden="true">
		<rect x="0" y="0" width="60" height="60" rx="6" />
		<path class="diag" d="M0,60L60,0" />
		<path class="curve" d={path} />
	</svg>
	<div class="body">
		<b>Pen feel</b>
		<div class="presets" role="radiogroup" aria-label="Pen feel">
			{#each PRESSURE_PRESETS as p (p.id)}
				<button
					type="button"
					role="radio"
					aria-checked={near(p.firmness)}
					onclick={() => setPressureFirmness(p.firmness)}
				>
					{p.label}
				</button>
			{/each}
		</div>
		<label class="ends">
			<span>Light</span>
			<input
				type="range"
				min="0.3"
				max="3"
				step="0.05"
				value={firmness}
				oninput={(e) => setPressureFirmness(Number(e.currentTarget.value))}
				aria-label="How hard you have to press"
			/>
			<span>Firm</span>
		</label>
		<span class="hint">Light: a soft touch already draws strong lines. Firm: press harder.</span>
	</div>
</section>

<style>
	.pen {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		padding: 10px;
		border-radius: 14px;
		background: var(--pp-ground);
	}
	svg {
		flex: 0 0 auto;
	}
	rect {
		fill: var(--pp-surface);
		stroke: var(--pp-track);
	}
	.diag {
		stroke: var(--pp-track);
		stroke-dasharray: 3 3;
		fill: none;
	}
	.curve {
		stroke: var(--pp-accent);
		stroke-width: 3;
		fill: none;
		stroke-linecap: round;
	}
	.body {
		display: flex;
		flex-direction: column;
		gap: 6px;
		flex: 1;
		min-width: 0;
	}
	b {
		font-size: 13px;
	}
	.presets {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.presets button {
		padding: 5px 10px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface);
		font: 600 12px var(--pp-font-body);
		cursor: pointer;
	}
	.presets button[aria-checked='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.ends {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		color: var(--pp-ink-2);
	}
	.ends input {
		flex: 1;
		min-width: 0;
		accent-color: var(--pp-accent);
	}
	.hint {
		font-size: 11px;
		color: var(--pp-muted);
	}
</style>
