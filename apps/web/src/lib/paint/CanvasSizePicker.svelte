<script lang="ts">
	import { CANVAS_PRESETS, checkSize, presetOf, type CanvasSize } from './canvas-size';

	let {
		size,
		blank,
		onpick
	}: {
		size: CanvasSize;
		/** nothing drawn yet: changing size needs no confirmation */
		blank: boolean;
		onpick: (s: CanvasSize) => void;
	} = $props();

	let open = $state(false);
	let choice = $state('');
	let customW = $state(0);
	let customH = $state(0);
	let confirming = $state(false);

	function toggle() {
		open = !open;
		if (open) {
			choice = presetOf(size);
			customW = size.width;
			customH = size.height;
			confirming = false;
		}
	}

	const picked = $derived.by(() => {
		if (choice === 'custom') return checkSize(Math.round(customW), Math.round(customH));
		const p = CANVAS_PRESETS.find((x) => x.id === choice);
		return p ? checkSize(p.width, p.height) : { ok: false as const, why: '' };
	});
	const same = $derived(
		picked.ok && picked.size.width === size.width && picked.size.height === size.height
	);

	function apply() {
		if (!picked.ok || same) return;
		if (!blank && !confirming) {
			confirming = true;
			return;
		}
		onpick(picked.size);
		open = false;
	}
</script>

<div class="picker">
	<button
		type="button"
		class="trigger"
		aria-expanded={open}
		aria-controls="canvas-size-panel"
		title="Canvas size"
		onclick={toggle}
	>
		<svg viewBox="0 0 24 24" aria-hidden="true">
			<rect x="4" y="6" width="16" height="12" rx="2" /><path d="M8 3v3M16 3v3M8 18v3M16 18v3" />
		</svg>
		<span>{size.width} × {size.height}</span>
	</button>
	{#if open}
		<div class="panel" id="canvas-size-panel" role="group" aria-label="Canvas size">
			<fieldset>
				<legend>Canvas size</legend>
				{#each CANVAS_PRESETS as p (p.id)}
					<label class="opt">
						<input type="radio" name="canvas-size" value={p.id} bind:group={choice} />
						<span class="shape" aria-hidden="true">
							<span
								style="width: {(p.width / Math.max(p.width, p.height)) * 22}px; height: {(p.height /
									Math.max(p.width, p.height)) *
									22}px"
							></span>
						</span>
						<span class="name">{p.label}</span>
						<span class="dims">{p.width}×{p.height}</span>
					</label>
				{/each}
				<label class="opt">
					<input type="radio" name="canvas-size" value="custom" bind:group={choice} />
					<span class="shape" aria-hidden="true"><span class="custom">?</span></span>
					<span class="name">Custom</span>
				</label>
				{#if choice === 'custom'}
					<div class="custom-row">
						<label>Width <input type="number" min="256" max="2048" bind:value={customW} /></label>
						<span aria-hidden="true">×</span>
						<label>Height <input type="number" min="256" max="2048" bind:value={customH} /></label>
					</div>
				{/if}
			</fieldset>
			{#if !picked.ok && picked.why}<p class="why" role="alert">{picked.why}</p>{/if}
			{#if confirming}
				<p class="warn" role="alert">
					A new size means a fresh, empty canvas. Your drawing will be cleared.
				</p>
			{/if}
			<div class="row">
				<button type="button" class="go" disabled={!picked.ok || same} onclick={apply}>
					{confirming ? 'Clear and resize' : 'Use this size'}
				</button>
				<button type="button" class="cancel" onclick={() => (open = false)}>Cancel</button>
			</div>
		</div>
	{/if}
</div>

<style>
	.picker {
		position: relative;
	}
	.trigger {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 48px;
		padding: 10px 16px;
		border: 0;
		border-radius: var(--pp-radius-btn);
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 15px var(--pp-font-body);
		cursor: pointer;
	}
	svg {
		width: 18px;
		height: 18px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
	}
	.panel {
		position: absolute;
		z-index: 20;
		top: calc(100% + 8px);
		right: 0;
		width: min(320px, calc(100vw - 32px));
		padding: 16px;
		border-radius: 20px;
		background: var(--pp-surface);
		box-shadow: 0 8px 30px rgba(22, 22, 26, 0.18);
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	fieldset {
		border: 0;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	legend {
		padding: 0 0 6px;
		font: 700 16px var(--pp-font-display);
	}
	.opt {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 40px;
		padding: 4px 8px;
		border-radius: 12px;
		cursor: pointer;
	}
	.opt:has(input:checked) {
		background: var(--pp-surface-sunk);
	}
	.opt input {
		accent-color: var(--pp-ink);
	}
	.shape {
		width: 24px;
		height: 24px;
		display: grid;
		place-items: center;
	}
	.shape span {
		display: block;
		border: 2px solid var(--pp-ink);
		border-radius: 3px;
	}
	.shape .custom {
		width: 18px;
		height: 18px;
		display: grid;
		place-items: center;
		font-size: 12px;
		font-weight: 700;
		border-style: dashed;
	}
	.name {
		flex: 1;
		font-weight: 600;
		font-size: 14px;
	}
	.dims {
		font-size: 12px;
		color: var(--pp-muted);
		font-variant-numeric: tabular-nums;
	}
	.custom-row {
		display: flex;
		align-items: end;
		gap: 8px;
		padding: 4px 8px;
	}
	.custom-row label {
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 12px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.custom-row input {
		width: 96px;
		min-height: 36px;
		padding: 4px 8px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 500 14px var(--pp-font-body);
	}
	.why,
	.warn {
		margin: 0;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
	.row {
		display: flex;
		gap: 8px;
	}
	.go,
	.cancel {
		min-height: 40px;
		padding: 8px 16px;
		border: 0;
		border-radius: 999px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.go {
		background: var(--pp-ink);
		color: #fff;
	}
	.go:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.cancel {
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
	}
</style>
