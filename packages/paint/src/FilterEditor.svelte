<script lang="ts">
	/**
	 * The Adjust panel's filters: pick one, set it with sliders. Changes go into
	 * `spec` and `onchange` runs so the canvas can preview them live.
	 */
	import {
		FILTERS,
		filterInfo,
		pickFilter,
		reseed,
		type FilterKind,
		type FilterSpec
	} from './filters';

	let {
		spec = $bindable(),
		onchange
	}: {
		spec: FilterSpec;
		onchange: () => void;
	} = $props();

	const info = $derived(filterInfo(spec.kind));

	function set(next: FilterSpec) {
		spec = next;
		onchange();
	}
</script>

<div class="filters">
	<label class="pick">
		<span>Filter</span>
		<select
			value={spec.kind}
			onchange={(e) => set(pickFilter(e.currentTarget.value as FilterKind, spec))}
		>
			{#each FILTERS as f (f.id)}
				<option value={f.id}>{f.label}</option>
			{/each}
		</select>
	</label>
	{#each info.sliders as s (s.key)}
		<label class="slider">
			<span>{s.label} <b>{Math.round(spec[s.key] * s.scale)}{s.unit}</b></span>
			<input
				type="range"
				min={s.min}
				max={s.max}
				step="1"
				value={Math.round(spec[s.key] * s.scale)}
				oninput={(e) => set({ ...spec, [s.key]: +e.currentTarget.value / s.scale })}
			/>
		</label>
	{/each}
	{#if info.mono}
		<div class="seg" role="radiogroup" aria-label="Colours">
			{#each [[false, 'Colour'], [true, 'Grey']] as const as [mono, label] (label)}
				<button
					type="button"
					class="tool"
					role="radio"
					aria-checked={spec.mono === mono}
					onclick={() => set({ ...spec, mono })}
				>
					{label}
				</button>
			{/each}
		</div>
	{/if}
	{#if info.seeded}
		<button type="button" class="tool" onclick={() => set(reseed(spec))}>New pattern</button>
	{/if}
</div>

<style>
	.filters {
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
	.pick {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-on-ink, #fff);
	}
	.pick select {
		height: 38px;
		border-radius: 10px;
		padding: 0 8px;
		background: #2a2a30;
		color: var(--pp-on-ink, #fff);
		border: 0;
		font: inherit;
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
	.slider input {
		accent-color: var(--pp-yellow);
	}
</style>
