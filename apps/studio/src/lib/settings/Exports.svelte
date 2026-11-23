<script lang="ts">
	/** Preferences: exports, and what the home screen opens and lists. */
	import { FORMATS } from '../exporting';
	import { EXPORT_FORMATS, settings, tidySettings } from '../settings.svelte';
	import './prefs.css';
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<span>Export → Animation starts on</span>
		<select bind:value={settings.exportFormat}>
			{#each EXPORT_FORMATS as id (id)}
				<option value={id}>{FORMATS.find((f) => f.id === id)?.label ?? id}</option>
			{/each}
		</select>
	</label>
	<label class="row">
		<span>Video quality</span>
		<select bind:value={settings.exportQuality}>
			<option value="best">Best (bigger files)</option>
			<option value="good">Good</option>
			<option value="small">Small files</option>
		</select>
	</label>
	<label class="row">
		<input type="checkbox" bind:checked={settings.exportCredits} />
		<span>
			Write credits into exports
			<small>(the app, your name and who drew it with you; not in GIFs)</small>
		</span>
	</label>
	<p class="hint">
		Colours: exports are sRGB and say so, so other apps show them as you see them here.
	</p>
	<label class="row">
		<input type="checkbox" bind:checked={settings.reopenLast} />
		<span>Open the last project when the app starts</span>
	</label>
	<label class="row">
		<span>Recent projects on the home screen</span>
		<input type="number" min="0" max="50" bind:value={settings.recentCount} />
	</label>
</div>
