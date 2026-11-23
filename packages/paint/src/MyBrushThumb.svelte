<script lang="ts">
	/** A small picture of one of My brushes, drawn with it (its settings, tip and grain). */
	import BrushPreview from './BrushPreview.svelte';
	import { brushSource } from './brushes';
	import { brushTexture, textureOn } from './brush-textures';
	import { applyTweaks, type CustomBrush } from './custom-brushes';

	let { brush }: { brush: CustomBrush } = $props();

	let myb = $state('');
	$effect(() => {
		const b = brush;
		let gone = false;
		void (b.source ? Promise.resolve(b.source) : brushSource(b.base))
			.then((src) => !gone && (myb = applyTweaks(src, b.tweaks)))
			.catch(() => {});
		return () => {
			gone = true;
		};
	});
	const texture = $derived.by(() => {
		const t = brushTexture(brush.id);
		return textureOn(t) ? t : null;
	});
</script>

<span class="thumb">
	{#if myb}
		<BrushPreview {myb} {texture} width={112} height={112} fit label="" />
	{/if}
</span>

<style>
	.thumb {
		display: block;
		width: 56px;
		height: 56px;
	}
	.thumb :global(canvas) {
		border: 0;
		border-radius: 10px;
	}
</style>
