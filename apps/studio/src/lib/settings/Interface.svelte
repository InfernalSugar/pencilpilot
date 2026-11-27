<script lang="ts">
	/** Preferences: how the app looks and where things are. */
	import { settings, tidySettings, UI_LAYOUTS, UI_SCALES } from '../settings.svelte';
	import ColourPick from './ColourPick.svelte';
	import './prefs.css';
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<span>Layout</span>
		<select bind:value={settings.uiLayout}>
			{#each UI_LAYOUTS as l (l.id)}
				<option value={l.id}>{l.label}</option>
			{/each}
		</select>
		<small>
			New: icon trays on the timeline, workspace tabs and right-click menus. Classic: every control
			in view, as before.
		</small>
	</label>
	<label class="row">
		<span>Size of everything</span>
		<select bind:value={settings.uiScale}>
			{#each UI_SCALES as k (k)}
				<option value={k}>{Math.round(k * 100)}%</option>
			{/each}
		</select>
	</label>
	<ColourPick label="Accent colour" bind:value={settings.accent} fallback="#e8492f" themed />
	<label class="row">
		<input type="checkbox" bind:checked={settings.leftHanded} />
		<span>Left-handed: tools on the right, panels on the left</span>
	</label>
	<label class="row">
		<input type="checkbox" bind:checked={settings.tooltips} />
		<span>
			Show tooltips <small>(the notes buttons show when the pointer rests on them)</small>
		</span>
	</label>
	<ColourPick label="Around the canvas" bind:value={settings.surround} fallback="#e9e6de" themed />
	<div class="row">
		<span>See-through areas:</span>
		<label class="row">
			<span>squares of</span>
			<input
				type="number"
				min="4"
				max="32"
				aria-label="Checkerboard square size"
				bind:value={settings.checkerSize}
			/>
			<span>px</span>
		</label>
		<ColourPick
			label="Checkerboard colour"
			bind:value={settings.checkerColour}
			fallback="#e9e6de"
		/>
		<span
			class="checker"
			aria-hidden="true"
			style="--c: {settings.checkerColour}; --s: {settings.checkerSize}px"
		></span>
	</div>
</div>

<style>
	.checker {
		width: 64px;
		height: 32px;
		border-radius: 8px;
		border: 1px solid var(--pp-surface-sunk);
		background: repeating-conic-gradient(var(--c) 0 25%, #fff 0 50%) 0 0 / var(--s) var(--s);
	}
</style>
