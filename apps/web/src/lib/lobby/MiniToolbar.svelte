<script lang="ts">
	import { onMount } from 'svelte';
	import { loadManifest, type BrushManifest } from '../paint/brushes';

	let {
		brushId = $bindable(),
		color = $bindable(),
		size = $bindable(),
		opacity = $bindable(),
		eraser = $bindable(),
		canUndo,
		onundo,
		onclear,
		clearLabel = 'Clear'
	}: {
		brushId: string;
		color: string;
		size: number;
		opacity: number;
		eraser: boolean;
		canUndo: boolean;
		onundo: () => void;
		onclear: () => void;
		clearLabel?: string;
	} = $props();

	const SWATCHES = ['#16161a', '#6b6a70', '#e8492f', '#3d6bff', '#1f9d74', '#f2a93b', '#8b5cf6'];
	let manifest = $state<BrushManifest | null>(null);
	onMount(() => {
		loadManifest().then((m) => (manifest = m));
	});
</script>

<div class="bar" role="toolbar" aria-label="Drawing tools">
	<label class="field">
		<span class="visually-hidden">Brush</span>
		<select bind:value={brushId} aria-label="Brush">
			{#if manifest}
				{#each manifest.categories as c (c.id)}
					<optgroup label={c.name}>
						{#each manifest.brushes.filter((b) => b.category === c.id) as b (b.id)}
							<option value={b.id}>{b.name}</option>
						{/each}
					</optgroup>
				{/each}
			{:else}
				<option value={brushId}>Loading brushes…</option>
			{/if}
		</select>
	</label>
	<button
		type="button"
		class="tool"
		aria-pressed={eraser}
		aria-label="Eraser"
		title="Eraser"
		onclick={() => (eraser = !eraser)}
	>
		<svg viewBox="0 0 24 24" aria-hidden="true">
			<path d="m7 21-4-4L14 6l5 5-8 10Z" /><path d="M22 21H7" />
		</svg>
	</button>
	<button
		type="button"
		class="tool"
		aria-label="Undo"
		title="Undo"
		disabled={!canUndo}
		onclick={onundo}
	>
		<svg viewBox="0 0 24 24" aria-hidden="true">
			<path d="M9 14 4 9l5-5" /><path d="M4 9h11a5 5 0 0 1 0 10h-3" />
		</svg>
	</button>
	<button type="button" class="tool text" onclick={onclear}>{clearLabel}</button>
	<label class="slider">
		<span>Size</span>
		<input
			type="range"
			min="-2"
			max="2.6"
			step="0.05"
			value={Math.log2(size)}
			oninput={(e) => (size = +(2 ** +e.currentTarget.value).toFixed(2))}
		/>
	</label>
	<label class="slider">
		<span>Opacity</span>
		<input type="range" min="0.05" max="1" step="0.05" bind:value={opacity} />
	</label>
	<div class="swatches" role="radiogroup" aria-label="Colour">
		{#each SWATCHES as c (c)}
			<button
				type="button"
				role="radio"
				aria-checked={color === c}
				aria-label="Colour {c}"
				class="swatch"
				style="background: {c}"
				onclick={() => (color = c)}
			></button>
		{/each}
		<label class="swatch custom" title="Pick any colour">
			<span class="visually-hidden">Custom colour</span>
			<input type="color" bind:value={color} />
		</label>
	</div>
</div>

<style>
	.bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
		padding: 12px;
		border-radius: var(--pp-radius-tile);
		background: var(--pp-ink);
		color: #fff;
	}
	select {
		min-height: 40px;
		max-width: 180px;
		padding: 4px 8px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: #fff;
		font: 600 14px var(--pp-font-body);
	}
	.tool {
		display: grid;
		place-items: center;
		min-width: 40px;
		height: 40px;
		padding: 0 10px;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: #fff;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tool[aria-pressed='true'] {
		background: var(--pp-accent-strong);
	}
	.tool:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	svg {
		width: 20px;
		height: 20px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.slider {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		font-weight: 600;
	}
	.slider input {
		width: 96px;
		accent-color: var(--pp-yellow);
	}
	.swatches {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.swatch {
		width: 28px;
		height: 28px;
		padding: 0;
		border: 2px solid #2a2a30;
		border-radius: 999px;
		cursor: pointer;
	}
	.swatch[aria-checked='true'] {
		outline: 3px solid var(--pp-yellow);
		outline-offset: 1px;
	}
	.custom {
		position: relative;
		overflow: hidden;
		background: conic-gradient(red, yellow, lime, aqua, blue, magenta, red);
	}
	.custom input {
		position: absolute;
		inset: 0;
		opacity: 0;
		cursor: pointer;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
