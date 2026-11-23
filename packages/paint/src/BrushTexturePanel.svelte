<script lang="ts">
	/**
	 * The current brush's texture: paper grain, a picture tip (its shape) and a
	 * dual brush. (Importing brushes is in the Brushes panel and the brush creator.)
	 */
	import {
		allTextures,
		brushTexture,
		deleteTexture,
		onTextures,
		setBrushTexture,
		textureOn,
		type Texture
	} from './brush-textures';
	import TexturePicker from './TexturePicker.svelte';

	let {
		brushId
	}: {
		brushId: string;
	} = $props();

	let version = $state(0);
	$effect(() => onTextures(() => version++));
	const tex = $derived.by(() => {
		void version;
		return brushTexture(brushId);
	});
	const mine = $derived.by(() => {
		void version;
		return allTextures().filter((t) => t.id.startsWith('img-'));
	});
	let open = $state(false);

	/** a small picture of a texture, for its button */
	function thumb(t: Texture): string {
		const c = document.createElement('canvas');
		c.width = t.w;
		c.height = t.h;
		const g = c.getContext('2d')!;
		const img = g.createImageData(t.w, t.h);
		for (let i = 0; i < t.px.length; i++) {
			const v = 255 - t.px[i];
			img.data.set([v, v, v, 255], i * 4);
		}
		g.putImageData(img, 0, 0);
		return c.toDataURL();
	}
</script>

<section class="tex" aria-label="Brush texture">
	<button type="button" class="head" aria-expanded={open} onclick={() => (open = !open)}>
		<b>Texture &amp; shape</b>
		<span>{textureOn(tex) ? 'On' : 'Off'}</span>
	</button>
	{#if open}
		<div class="body">
			<TexturePicker value={tex} onchange={(t) => setBrushTexture(brushId, t)} />
			{#if mine.length}
				<details>
					<summary>Your pictures</summary>
					<div class="pics">
						{#each mine as t (t.id)}
							<button
								type="button"
								class="pic"
								title="Delete {t.name}"
								aria-label="Delete picture {t.name}"
								onclick={() => deleteTexture(t.id)}
							>
								<img src={thumb(t)} alt="" /><span class="x" aria-hidden="true">×</span>
							</button>
						{/each}
					</div>
				</details>
			{/if}
		</div>
	{/if}
</section>

<style>
	.tex {
		margin-top: 12px;
		border-top: 1px solid var(--pp-line, #e3e3e8);
		padding-top: 8px;
	}
	.head {
		display: flex;
		justify-content: space-between;
		width: 100%;
		min-height: 44px;
		padding: 8px 4px;
		border: 0;
		background: none;
		color: inherit;
		font: inherit;
		cursor: pointer;
	}
	.head span {
		color: var(--pp-muted, #6b6b76);
	}
	details {
		margin-top: 12px;
	}
	.pics {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-top: 6px;
	}
	.pic {
		position: relative;
		display: grid;
		place-items: center;
		width: 48px;
		height: 48px;
		padding: 0;
		overflow: hidden;
		border: 2px solid var(--pp-line, #e3e3e8);
		border-radius: 10px;
		background: #fff;
		cursor: pointer;
	}
	.pic img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.x {
		position: absolute;
		top: 0;
		right: 3px;
		color: #c00;
		font-size: 16px;
	}
</style>
