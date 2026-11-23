<script lang="ts">
	/**
	 * Choose a brush's textures: paper grain, a picture tip (its shape) and a dual
	 * brush. It only reports the new choice (`onchange`): whoever shows it keeps it.
	 */
	import {
		allTextures,
		onTextures,
		saveTexture,
		type BrushTexture,
		type Texture
	} from './brush-textures';
	import { importBrushFile } from './brush-import';
	import { decodeImage } from './my-brush-import';

	let {
		value,
		onchange
	}: {
		value: BrushTexture;
		onchange: (t: BrushTexture) => void;
	} = $props();

	let version = $state(0);
	$effect(() => onTextures(() => version++));
	const textures = $derived.by(() => {
		void version;
		return allTextures();
	});
	const grains = $derived(textures.filter((t) => t.kind === 'grain'));
	const tips = $derived(textures.filter((t) => t.kind === 'tip'));
	let note = $state('');

	const set = (patch: Partial<BrushTexture>) => onchange({ ...value, ...patch });

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
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a cache of data URLs, never rendered on its own
	const thumbs = new Map<string, string>();
	const thumbOf = (t: Texture) => {
		let u = thumbs.get(t.id);
		if (!u) thumbs.set(t.id, (u = thumb(t)));
		return u;
	};

	/** Any picture as a new grain or tip. */
	async function addPicture(files: FileList | null, kind: 'grain' | 'tip') {
		const f = files?.[0];
		if (!f) return;
		try {
			const [b] = await importBrushFile(
				new File([f], f.name.replace(/\.[^.]+$/, '') + '.png', { type: f.type }),
				decodeImage
			);
			if (!b.tip) throw new Error('no picture');
			const t = saveTexture({ name: b.name, kind, ...b.tip });
			set(kind === 'grain' ? { grain: t.id } : { tip: t.id });
			note = '';
		} catch {
			note = 'That picture couldn’t be read.';
		}
	}
</script>

{#snippet pics(
	label: string,
	list: Texture[],
	chosen: string | null,
	none: string,
	pick: (id: string | null) => void,
	add?: 'grain' | 'tip'
)}
	<div class="pics" role="radiogroup" aria-label={label}>
		<button
			type="button"
			class="pic none"
			role="radio"
			aria-checked={!chosen}
			onclick={() => pick(null)}
		>
			{none}
		</button>
		{#each list as t (t.id)}
			<button
				type="button"
				class="pic"
				role="radio"
				aria-checked={chosen === t.id}
				aria-label="{label}: {t.name}"
				title={t.name}
				onclick={() => pick(t.id)}
			>
				<img src={thumbOf(t)} alt="" />
			</button>
		{/each}
		{#if add}
			<label
				class="pic add"
				title={add === 'tip'
					? 'Use a picture as the brush shape (dark = paint)'
					: 'Use a picture as grain'}
			>
				+<input
					type="file"
					accept="image/*"
					aria-label="Add a picture as {add === 'tip' ? 'a tip' : 'grain'}"
					onchange={(e) => addPicture(e.currentTarget.files, add)}
				/>
			</label>
		{/if}
	</div>
{/snippet}

{#snippet slider(
	label: string,
	v: number,
	min: number,
	max: number,
	step: number,
	show: string,
	put: (v: number) => void
)}
	<label class="slider">
		<span>{label} <b>{show}</b></span>
		<input type="range" {min} {max} {step} value={v} oninput={(e) => put(+e.currentTarget.value)} />
	</label>
{/snippet}

<div class="textures">
	<h4>Tip <small>(the brush’s shape)</small></h4>
	{@render pics('Tip', tips, value.tip, 'Round', (id) => set({ tip: id }), 'tip')}

	<h4>Grain <small>(paper texture: paint skips its low spots)</small></h4>
	{@render pics('Grain', grains, value.grain, 'None', (id) => set({ grain: id }), 'grain')}
	{#if value.grain}
		{@render slider(
			'Strength',
			value.grainStrength,
			0,
			1,
			0.05,
			`${Math.round(value.grainStrength * 100)}%`,
			(v) => set({ grainStrength: v })
		)}
		{@render slider(
			'Scale',
			value.grainScale,
			0.3,
			4,
			0.1,
			`${value.grainScale.toFixed(1)}×`,
			(v) => set({ grainScale: v })
		)}
	{/if}

	<h4>Dual brush <small>(a second shape: paint only where both meet)</small></h4>
	{@render pics('Dual brush', tips, value.dual, 'None', (id) => set({ dual: id }))}
	{#if value.dual}
		{@render slider(
			'Size',
			value.dualSize,
			0.1,
			1.5,
			0.05,
			`${Math.round(value.dualSize * 100)}%`,
			(v) => set({ dualSize: v })
		)}
		{@render slider(
			'Scatter',
			value.dualScatter,
			0,
			1.5,
			0.05,
			`${Math.round(value.dualScatter * 100)}%`,
			(v) => set({ dualScatter: v })
		)}
		{@render slider('Count', value.dualCount, 1, 8, 1, `${value.dualCount}`, (v) =>
			set({ dualCount: v })
		)}
	{/if}
	{#if note}<p class="note" role="status">{note}</p>{/if}
</div>

<style>
	h4 {
		margin: 12px 0 6px;
		font-size: 14px;
	}
	h4:first-child {
		margin-top: 0;
	}
	small {
		font-weight: 400;
		color: var(--pp-muted, #6b6b76);
	}
	.pics {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.pic {
		position: relative;
		display: grid;
		place-items: center;
		width: 48px;
		height: 48px;
		padding: 0;
		overflow: hidden;
		border: 2px solid var(--pp-track, #e3e3e8);
		border-radius: 10px;
		background: #fff;
		color: #16161a;
		font: 600 12px var(--pp-font-body, sans-serif);
		cursor: pointer;
	}
	.pic[aria-checked='true'] {
		border-color: var(--pp-accent, #2f6bff);
		box-shadow: 0 0 0 2px var(--pp-accent, #2f6bff);
	}
	.pic img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.pic input {
		position: absolute;
		opacity: 0;
		width: 1px;
		height: 1px;
	}
	.add {
		font-size: 22px;
	}
	.slider {
		display: grid;
		gap: 2px;
		margin-top: 6px;
	}
	.slider span {
		display: flex;
		justify-content: space-between;
		font-size: 13px;
	}
	.slider input {
		accent-color: var(--pp-accent-strong, #2f6bff);
	}
	.note {
		margin: 6px 0;
		font-size: 13px;
		color: var(--pp-muted, #6b6b76);
	}
</style>
