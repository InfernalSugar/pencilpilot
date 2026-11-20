<script lang="ts" module>
	import type { TraceTransform } from './trace';

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
	import { loadOwnPicture } from './trace';

	let {
		trace = $bindable(),
		hasReference,
		onclose,
		onhide
	}: {
		trace: TraceState;
		hasReference: boolean;
		onclose: () => void;
		/** tuck the bar away; tracing stays on */
		onhide?: () => void;
	} = $props();

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
	<div class="row">
		<span
			class="label"
			title="Traced drawings get tips but no score, and don’t count toward your progress."
		>
			Tracing <small>no score</small>
		</span>
		{#if onhide}
			<button
				type="button"
				class="close"
				aria-label="Hide the tracing settings"
				title="Hide these settings (tracing stays on; the Trace button shows them again)"
				onclick={onhide}
			>
				▴
			</button>
		{/if}
		<button
			type="button"
			class="close"
			aria-label="Stop tracing"
			title="Stop tracing"
			onclick={onclose}
		>
			×
		</button>
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
			{trace.ownUrl ? 'Another picture…' : 'Use my own picture…'}
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
		<label class="mini">
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
		<label class="mini">
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
		<label class="mini">
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
		{#if placed}
			<button type="button" class="chip" onclick={resetPlacement}>Reset</button>
		{/if}
	</div>
	{#if error}<p class="note err" role="alert">{error}</p>{/if}
	{#if trace.adjusting}
		<p class="note" role="status">Drag on the canvas to line the picture up. Drawing is paused.</p>
	{/if}
</section>

<style>
	/* one compact row, like the options bar (it scrolls sideways if the window is narrow) */
	.trace {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
		color: var(--pp-on-ink);
	}
	.row {
		display: flex;
		align-items: center;
		gap: 8px 12px;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.row > * {
		flex: none;
	}
	.label {
		font: 700 15px var(--pp-font-display);
		white-space: nowrap;
	}
	.label small {
		margin-left: 4px;
		padding: 2px 6px;
		border-radius: 999px;
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font: 700 11px var(--pp-font-body);
	}
	.seg {
		display: flex;
		gap: 2px;
		padding: 2px;
		border-radius: 12px;
		background: #2a2a30;
	}
	.seg button,
	.chip,
	.close {
		height: 34px;
		padding: 0 12px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		white-space: nowrap;
		cursor: pointer;
	}
	.seg button[aria-checked='true'],
	.chip[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.chip {
		background: #2a2a30;
	}
	.close {
		width: 34px;
		padding: 0;
		background: #2a2a30;
		font-size: 20px;
		line-height: 1;
	}
	.mini {
		display: flex;
		flex-direction: column;
		gap: 2px;
		width: 110px;
		font-size: 11px;
		color: var(--pp-on-ink-muted);
	}
	.mini b {
		color: var(--pp-on-ink);
	}
	.mini input {
		accent-color: var(--pp-yellow);
	}
	.note {
		margin: 0;
		font-size: 12px;
		color: var(--pp-on-ink-muted);
	}
	.err {
		color: #ffb4a4;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
