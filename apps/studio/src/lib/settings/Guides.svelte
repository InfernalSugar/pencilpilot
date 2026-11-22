<script lang="ts">
	/** Preferences: guides, the onion skin, and how new projects start. */
	import { DEFAULT_BRUSH, loadManifest, type BrushManifest } from '@pencilpilot/paint/brushes';
	import { onMount } from 'svelte';
	import { ONION_DEFAULTS } from '../onion';
	import { settings, tidySettings } from '../settings.svelte';
	import ColourPick from './ColourPick.svelte';
	import './prefs.css';

	let manifest = $state<BrushManifest | null>(null);
	onMount(() => {
		loadManifest()
			.then((m) => (manifest = m))
			.catch(() => {});
	});
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<span>Grid size on new canvases</span>
		<input type="number" min="0" max="2000" bind:value={settings.gridSize} />
		<small>px (0: an eighth of the canvas)</small>
	</label>
	<ColourPick label="Grid colour" bind:value={settings.gridColour} fallback="#3d6bff" />
	<ColourPick
		label="Perspective guide colour"
		bind:value={settings.guideColour}
		fallback="#e8492f"
	/>
	<fieldset>
		<legend>Onion skin</legend>
		<label class="row">
			<span>Drawings before</span>
			<input type="number" min="0" max="5" bind:value={settings.onion.before} />
			<span>and after</span>
			<input
				type="number"
				min="0"
				max="5"
				aria-label="Drawings after"
				bind:value={settings.onion.after}
			/>
		</label>
		<ColourPick
			label="Before in"
			bind:value={settings.onion.beforeColour}
			fallback={ONION_DEFAULTS.beforeColour}
		/>
		<ColourPick
			label="After in"
			bind:value={settings.onion.afterColour}
			fallback={ONION_DEFAULTS.afterColour}
		/>
	</fieldset>
	<fieldset>
		<legend>New projects</legend>
		<label class="row">
			<span>Width</span>
			<input type="number" min="64" max="8192" bind:value={settings.newWidth} />
			<span>Height</span>
			<input
				type="number"
				min="64"
				max="8192"
				aria-label="Height"
				bind:value={settings.newHeight}
			/>
			<small>px</small>
		</label>
		<label class="row">
			<span>Frames a second</span>
			<input type="number" min="1" max="60" bind:value={settings.newFps} />
		</label>
		<label class="row">
			<span>Brush</span>
			<select bind:value={settings.defaultBrush}>
				<option value="">
					The usual ({manifest?.brushes.find((b) => b.id === DEFAULT_BRUSH)?.name ?? 'pencil'})
				</option>
				{#each manifest?.brushes ?? [] as b (b.id)}
					<option value={b.id}>{b.name}</option>
				{/each}
			</select>
		</label>
	</fieldset>
</div>
