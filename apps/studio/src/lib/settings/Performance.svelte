<script lang="ts">
	/** Preferences: undo, the memory frames may take, and where the rest goes. */
	import { isTauri } from '@tauri-apps/api/core';
	import { settings, tidySettings } from '../settings.svelte';
	import './prefs.css';

	const gb = $derived(+(settings.frameMemoryMb / 1024).toFixed(2));

	async function chooseFolder() {
		const { open } = await import('@tauri-apps/plugin-dialog');
		const f = await open({
			directory: true,
			multiple: false,
			defaultPath: settings.framesFolder ?? undefined
		});
		if (typeof f === 'string') settings.framesFolder = f;
	}
</script>

<div class="pp-prefs" onchange={tidySettings}>
	<label class="row">
		<span>Undo steps</span>
		<input type="number" min="10" max="500" bind:value={settings.undoSteps} />
		<small>10–500; more takes more memory</small>
	</label>
	<label class="row">
		<span>Memory for frames, at most</span>
		<input
			type="number"
			min="0.25"
			max="256"
			step="0.25"
			value={gb}
			oninput={(e) => {
				const v = +e.currentTarget.value;
				if (v > 0) settings.frameMemoryMb = Math.round(v * 1024);
			}}
		/>
		<span>GB</span>
	</label>
	<p class="hint">
		An animation’s frames never take more than this: past it, the ones not in use go to disk and
		come back when needed, so work never stops.
	</p>
	<div class="row">
		<span>Frames past it go to: <b>{settings.framesFolder ?? 'the app’s cache folder'}</b></span>
		{#if isTauri()}
			<button type="button" class="small" onclick={chooseFolder}>Choose…</button>
		{/if}
		{#if settings.framesFolder}
			<button type="button" class="small" onclick={() => (settings.framesFolder = null)}>
				Use the cache folder
			</button>
		{/if}
	</div>
</div>
