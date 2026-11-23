<script lang="ts">
	/** A colour preference: a colour well, and (when it can be) a way back to the default. */
	let {
		label,
		value = $bindable(),
		fallback,
		themed = false
	}: {
		label: string;
		value: string;
		/** what the well shows when nothing is chosen, and what Reset goes back to */
		fallback: string;
		/** '' means the theme's own colour */
		themed?: boolean;
	} = $props();
	const reset = $derived(themed ? '' : fallback);

	/** following the theme: show the colour the theme really uses now (dark or light) */
	let themeColour = $state('');
	$effect(() => {
		if (!themed || value) return;
		const read = () => {
			const v = getComputedStyle(document.documentElement)
				.getPropertyValue('--pp-surface-sunk')
				.trim();
			themeColour = /^#[0-9a-f]{6}$/i.test(v) ? v : '';
		};
		read();
		// the theme can change while this is open (Settings → Appearance, or the system's)
		const watch = new MutationObserver(read);
		watch.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
		const dark = matchMedia('(prefers-color-scheme: dark)');
		dark.addEventListener('change', read);
		return () => {
			watch.disconnect();
			dark.removeEventListener('change', read);
		};
	});
</script>

<label class="row">
	<span>{label}</span>
	<input
		type="color"
		value={value || themeColour || fallback}
		oninput={(e) => (value = e.currentTarget.value)}
	/>
	{#if value !== reset}
		<button type="button" class="small" onclick={() => (value = reset)}>
			{themed ? 'Use the theme’s' : 'Reset'}
		</button>
	{:else if themed}
		<span class="swatch-off">The theme’s</span>
	{/if}
</label>
