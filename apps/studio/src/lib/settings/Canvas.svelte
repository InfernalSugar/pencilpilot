<script lang="ts">
	/** Preferences: zooming, the scroll wheel, rulers and units. */
	import { settings, tidySettings, ZOOM_SPEEDS } from '../settings.svelte';
	import './prefs.css';
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<input type="checkbox" bind:checked={settings.zoomToPointer} />
		<span>Zoom towards the pointer <small>(off: towards the middle)</small></span>
	</label>
	<fieldset>
		<legend>The scroll wheel</legend>
		<label class="row">
			<input type="radio" name="wheel" value="scroll" bind:group={settings.wheel} />
			<span>Moves the view (Ctrl zooms)</span>
		</label>
		<label class="row">
			<input type="radio" name="wheel" value="zoom" bind:group={settings.wheel} />
			<span>Zooms (Ctrl moves the view)</span>
		</label>
	</fieldset>
	<label class="row">
		<span>Zoom speed</span>
		<select bind:value={settings.zoomSpeed}>
			{#each ZOOM_SPEEDS as k (k)}
				<option value={k}>{k === 1 ? 'Normal' : `${k}×`}</option>
			{/each}
		</select>
	</label>
	<fieldset>
		<legend>Zooming feels</legend>
		<label class="row">
			<input type="radio" name="zoom-feel" value={true} bind:group={settings.smoothZoom} />
			<span>Smooth <small>(glides a little at a time)</small></span>
		</label>
		<label class="row">
			<input type="radio" name="zoom-feel" value={false} bind:group={settings.smoothZoom} />
			<span>Snappy <small>(jumps straight to each step)</small></span>
		</label>
	</fieldset>
	<label class="row">
		<input type="checkbox" bind:checked={settings.rulers} />
		<span>Rulers along the canvas</span>
	</label>
	<label class="row">
		<span>Units</span>
		<select bind:value={settings.units}>
			<option value="px">Pixels</option>
			<option value="cm">Centimetres</option>
			<option value="in">Inches</option>
		</select>
	</label>
	{#if settings.units !== 'px'}
		<label class="row">
			<span>Pixels per inch</span>
			<input type="number" min="36" max="1200" bind:value={settings.ppi} />
		</label>
	{/if}
	<p class="hint">Units are used by the rulers, new canvas sizes and the measuring tool.</p>
</div>
