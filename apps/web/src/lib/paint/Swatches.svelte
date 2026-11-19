<script lang="ts" module>
	export const SWATCHES = [
		'#16161a',
		'#6b6a70',
		'#b9b6ae',
		'#e8492f',
		'#3d6bff',
		'#1f9d74',
		'#f2a93b'
	];
</script>

<script lang="ts">
	/** Quick colours for the options bar (drag one onto the canvas to fill there). */
	let {
		color = $bindable(),
		onmore,
		moreOpen = false
	}: {
		color: string;
		/** "More colours" (none: no button) */
		onmore?: () => void;
		moreOpen?: boolean;
	} = $props();
</script>

<div class="swatches" role="radiogroup" aria-label="Colour">
	{#each SWATCHES as c (c)}
		<button
			type="button"
			role="radio"
			aria-checked={color === c}
			aria-label="Colour {c}"
			draggable="true"
			ondragstart={(e) => e.dataTransfer?.setData('application/x-pp-color', c)}
			class="swatch"
			style="background: {c}"
			onclick={() => (color = c)}
		></button>
	{/each}
	{#if onmore}
		<button
			type="button"
			class="swatch custom"
			aria-label="More colours"
			aria-expanded={moreOpen}
			title="Colour wheel, recent colours and palettes"
			onclick={onmore}
		></button>
	{/if}
</div>

<style>
	.swatches {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.swatch {
		flex: none;
		width: 30px;
		height: 30px;
		border-radius: 999px;
		border: 2px solid #2a2a30;
		cursor: pointer;
		padding: 0;
	}
	.swatch[aria-checked='true'] {
		outline: 3px solid var(--pp-yellow);
		outline-offset: 1px;
	}
	.custom {
		background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
	}
</style>
