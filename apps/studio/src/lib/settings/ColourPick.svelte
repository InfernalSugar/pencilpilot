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
</script>

<label class="row">
	<span>{label}</span>
	<input type="color" value={value || fallback} oninput={(e) => (value = e.currentTarget.value)} />
	{#if value !== reset}
		<button type="button" class="small" onclick={() => (value = reset)}>
			{themed ? 'Use the theme’s' : 'Reset'}
		</button>
	{:else if themed}
		<span class="swatch-off">The theme’s</span>
	{/if}
</label>
