<script lang="ts">
	/** Preferences: the brush cursor. */
	import { settings, tidySettings } from '../settings.svelte';
	import './prefs.css';

	const LOOKS = [
		['outline', 'The brush’s outline, at its real size'],
		['crosshair', 'A crosshair'],
		['dot', 'A dot']
	] as const;
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<fieldset>
		<legend>The brush cursor</legend>
		{#each LOOKS as [id, text] (id)}
			<label class="row">
				<input type="radio" name="cursor" value={id} bind:group={settings.cursor} />
				<span>{text}</span>
			</label>
		{/each}
	</fieldset>
	<label class="row">
		<input type="checkbox" bind:checked={settings.cursorWhilePainting} />
		<span>Show the cursor while painting</span>
	</label>
	<label class="row">
		<input
			type="checkbox"
			bind:checked={settings.crosshairWhenSmall}
			disabled={settings.cursor !== 'outline'}
		/>
		<span>A crosshair when the brush is too small to outline <small>(off: a dot)</small></span>
	</label>
</div>
