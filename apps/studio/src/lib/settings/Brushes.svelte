<script lang="ts">
	/** Preferences: brush size and what each brush remembers. */
	import { BRUSH_SIZE_TOP, settings, tidySettings } from '../settings.svelte';
	import './prefs.css';

	const TOPS = [1, 1.5, 2, 3, 4, BRUSH_SIZE_TOP];
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<input type="checkbox" bind:checked={settings.brushFollowsZoom} />
		<span>
			The brush keeps its size on screen when you zoom
			<small>(smaller on the canvas when zoomed in, so it stays the same size on screen)</small>
		</span>
	</label>
	<label class="row">
		<span>Largest brush size</span>
		<select bind:value={settings.maxBrushSize}>
			{#each TOPS as k (k)}
				<option value={k}>{Math.round(k * 100)}%</option>
			{/each}
		</select>
	</label>
	<label class="row">
		<input type="checkbox" bind:checked={settings.perBrushSize} />
		<span>Each brush remembers its own size and opacity</span>
	</label>
</div>
