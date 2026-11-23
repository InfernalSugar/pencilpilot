<script lang="ts">
	/** Preferences: how a selection shows. */
	import { settings, tidySettings } from '../settings.svelte';
	import './prefs.css';
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<span>Shade outside the selection <b>{Math.round(settings.selectionTint * 100)}%</b></span>
		<input type="range" min="0" max="0.8" step="0.05" bind:value={settings.selectionTint} />
	</label>
	<label class="row">
		<input type="checkbox" bind:checked={settings.marchingAnts} />
		<span>Marching ants <small>(off: the outline stays still)</small></span>
	</label>
</div>
