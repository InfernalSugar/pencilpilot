<script lang="ts">
	/**
	 * A small picture of a brush from the pack: a stroke drawn with it (its size, grain
	 * and texture), not a picture of the pen. Drawn once it scrolls into view; until
	 * then, the pack's own picture.
	 */
	import BrushPreview from './BrushPreview.svelte';
	import { brushSource, previewUrl } from './brushes';
	import { brushTexture, textureOn } from './brush-textures';

	let { id, picture = true }: { id: string; picture?: boolean } = $props();

	let el = $state<HTMLElement>();
	let seen = $state(false);
	let myb = $state('');

	$effect(() => {
		if (!el || seen) return;
		const io = new IntersectionObserver(
			(entries) => {
				if (entries.some((e) => e.isIntersecting)) {
					seen = true;
					io.disconnect();
				}
			},
			{ rootMargin: '200px' }
		);
		io.observe(el);
		return () => io.disconnect();
	});

	$effect(() => {
		if (!seen) return;
		const b = id;
		let gone = false;
		void brushSource(b)
			.then((src) => !gone && (myb = src))
			.catch(() => {});
		return () => {
			gone = true;
		};
	});

	const texture = $derived.by(() => {
		const t = brushTexture(id);
		return textureOn(t) ? t : null;
	});
</script>

<span class="thumb" bind:this={el} data-brush-thumb={id}>
	{#if myb}
		<BrushPreview {myb} {texture} width={112} height={112} fit label="" />
	{:else if picture}
		<img src={previewUrl(id)} alt="" loading="lazy" width="56" height="56" />
	{/if}
</span>

<style>
	.thumb {
		display: block;
		width: 56px;
		height: 56px;
	}
	.thumb :global(canvas) {
		width: 56px;
		height: 56px;
		border: 0;
		border-radius: 10px;
		background: #fff;
	}
	.thumb img {
		width: 56px;
		height: 56px;
		object-fit: contain;
		mix-blend-mode: multiply;
	}
	:global([data-theme='dark']) .thumb img {
		mix-blend-mode: normal;
		background: #fff;
		border-radius: 10px;
	}
</style>
