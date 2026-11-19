<script lang="ts" module>
	import type { TraceTransform } from '../references';

	export interface TraceState extends TraceTransform {
		source: 'reference' | 'own';
		/** the student's own picture (a data URL; it never leaves the browser) */
		ownUrl: string | null;
		opacity: number;
		/** dragging on the canvas moves the picture */
		adjusting: boolean;
	}

	export const TRACE_DEFAULTS: TraceState = {
		source: 'reference',
		ownUrl: null,
		opacity: 0.35,
		x: 0,
		y: 0,
		scale: 1,
		rotate: 0,
		adjusting: false
	};
</script>

<script lang="ts">
	import { loadOwnPicture } from '../references';

	let {
		trace = $bindable(),
		hasReference,
		onclose
	}: { trace: TraceState; hasReference: boolean; onclose: () => void } = $props();

	let error = $state('');
	let fileInput: HTMLInputElement;

	async function pick(e: Event) {
		const file = (e.currentTarget as HTMLInputElement).files?.[0];
		if (!file) return;
		error = '';
		try {
			trace = {
				...trace,
				source: 'own',
				ownUrl: await loadOwnPicture(file),
				x: 0,
				y: 0,
				scale: 1,
				rotate: 0
			};
		} catch {
			error = 'That file isn’t a picture this browser can open.';
		}
	}

	const resetPlacement = () => (trace = { ...trace, x: 0, y: 0, scale: 1, rotate: 0 });
	const placed = $derived(
		trace.x !== 0 || trace.y !== 0 || trace.scale !== 1 || trace.rotate !== 0
	);
</script>

<section class="trace" aria-label="Tracing">
	<div class="head">
		<h2>Tracing</h2>
		<p>Traced drawings get tips but no score, and don’t count toward your progress.</p>
		<button type="button" class="close" onclick={onclose}>Stop tracing</button>
	</div>

	<div class="row">
		{#if hasReference}
			<div class="seg" role="radiogroup" aria-label="Picture to trace">
				<button
					type="button"
					role="radio"
					aria-checked={trace.source === 'reference'}
					onclick={() => (trace = { ...trace, source: 'reference' })}
				>
					Reference
				</button>
				<button
					type="button"
					role="radio"
					aria-checked={trace.source === 'own'}
					onclick={() => (trace.ownUrl ? (trace = { ...trace, source: 'own' }) : fileInput.click())}
				>
					My picture
				</button>
			</div>
		{/if}
		<button type="button" class="chip" onclick={() => fileInput.click()}>
			{trace.ownUrl ? 'Choose another picture…' : 'Use my own picture…'}
		</button>
		<input
			bind:this={fileInput}
			class="visually-hidden"
			type="file"
			accept="image/*"
			aria-label="Picture from your device"
			onchange={pick}
		/>
		<button
			type="button"
			class="chip"
			aria-pressed={trace.adjusting}
			title="Drag on the canvas to move the picture"
			onclick={() => (trace = { ...trace, adjusting: !trace.adjusting })}
		>
			{trace.adjusting ? 'Done moving' : 'Move picture'}
		</button>
		{#if placed}
			<button type="button" class="chip" onclick={resetPlacement}>Reset position</button>
		{/if}
	</div>
	{#if error}<p class="err" role="alert">{error}</p>{/if}
	{#if trace.adjusting}
		<p class="hint" role="status">Drag on the canvas to line the picture up. Drawing is paused.</p>
	{/if}

	<div class="sliders">
		<label>
			<span>Opacity <b>{Math.round(trace.opacity * 100)}%</b></span>
			<input
				type="range"
				min="5"
				max="90"
				step="1"
				value={Math.round(trace.opacity * 100)}
				oninput={(e) => (trace = { ...trace, opacity: +e.currentTarget.value / 100 })}
			/>
		</label>
		<label>
			<span>Size <b>{Math.round(trace.scale * 100)}%</b></span>
			<input
				type="range"
				min="25"
				max="300"
				step="1"
				value={Math.round(trace.scale * 100)}
				oninput={(e) => (trace = { ...trace, scale: +e.currentTarget.value / 100 })}
			/>
		</label>
		<label>
			<span>Rotate <b>{Math.round(trace.rotate)}°</b></span>
			<input
				type="range"
				min="-180"
				max="180"
				step="1"
				value={trace.rotate}
				oninput={(e) => (trace = { ...trace, rotate: +e.currentTarget.value })}
			/>
		</label>
	</div>
</section>

<style>
	.trace {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 20px;
		border-radius: var(--pp-radius-tile);
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 16px;
	}
	h2 {
		margin: 0;
		font-size: 20px;
		color: var(--pp-ink);
	}
	.head p {
		flex: 1 1 260px;
		margin: 0;
		font-size: 14px;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		align-items: center;
	}
	.seg {
		display: flex;
		gap: 2px;
		padding: 3px;
		border-radius: 999px;
		background: var(--pp-surface);
	}
	.seg button,
	.chip,
	.close {
		min-height: 36px;
		padding: 6px 14px;
		border: 0;
		border-radius: 999px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
		color: var(--pp-ink);
	}
	.seg button {
		background: transparent;
	}
	.seg button[aria-checked='true'],
	.chip[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.chip {
		background: var(--pp-surface);
	}
	.close {
		background: var(--pp-ink);
		color: #fff;
	}
	.sliders {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
		gap: 10px 16px;
	}
	.sliders label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink);
	}
	input[type='range'] {
		accent-color: var(--pp-ink);
	}
	.hint,
	.err {
		margin: 0;
		font-size: 14px;
		font-weight: 600;
		color: var(--pp-ink);
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
