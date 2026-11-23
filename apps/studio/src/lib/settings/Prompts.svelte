<script lang="ts">
	/** Preferences: the questions the app asks first, and its sound. */
	import { settings, tidySettings } from '../settings.svelte';
	import './prefs.css';
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<input type="checkbox" bind:checked={settings.askToSave} />
		<span>Ask “Save before…?” when unsaved work would be left</span>
	</label>
	{#if !settings.askToSave}
		<p class="hint">
			Without asking, unsaved work is kept as a recovery copy: the home screen offers it back.
		</p>
	{/if}
	<label class="row">
		<input type="checkbox" bind:checked={settings.askToDelete} />
		<span>Ask before deleting one of My brushes</span>
	</label>
	<label class="row">
		<input type="checkbox" bind:checked={settings.scrubSound} />
		<span>Play a snippet of the sound when stepping through frames</span>
	</label>
</div>
