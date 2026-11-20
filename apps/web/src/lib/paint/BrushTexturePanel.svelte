<script lang="ts">
	/**
	 * The current brush's texture: paper grain, a picture tip (its shape) and a
	 * dual brush, plus importing brushes from MyPaint, Photoshop and Procreate.
	 */
	import {
		allTextures,
		brushTexture,
		deleteTexture,
		NO_TEXTURE,
		onTextures,
		saveTexture,
		setBrushTexture,
		textureOn,
		type BrushTexture,
		type Texture
	} from './brush-textures';
	import { importBrushFile, type ImportedBrush } from './brush-import';
	import { saveCustomBrush } from './custom-brushes';

	let {
		brushId,
		onpick
	}: {
		brushId: string;
		/** an imported brush to switch to */
		onpick?: (id: string) => void;
	} = $props();

	let version = $state(0);
	$effect(() => onTextures(() => version++));
	const tex = $derived.by(() => {
		void version;
		return brushTexture(brushId);
	});
	const textures = $derived.by(() => {
		void version;
		return allTextures();
	});
	const grains = $derived(textures.filter((t) => t.kind === 'grain'));
	const tips = $derived(textures.filter((t) => t.kind === 'tip'));
	let open = $state(false);
	let note = $state('');
	let busy = $state(false);

	function set(patch: Partial<BrushTexture>) {
		setBrushTexture(brushId, { ...tex, ...patch });
	}

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

	async function decode(bytes: Uint8Array): Promise<ImageData> {
		const bmp = await createImageBitmap(new Blob([bytes as BlobPart]));
		const c = new OffscreenCanvas(bmp.width, bmp.height);
		const g = c.getContext('2d')!;
		g.drawImage(bmp, 0, 0);
		bmp.close();
		return g.getImageData(0, 0, c.width, c.height);
	}

	/** Bring brushes in: each becomes one of "My brushes" (with its pictures as textures). */
	async function importFiles(files: FileList | null) {
		if (!files?.length) return;
		busy = true;
		note = '';
		let made = 0;
		let first = '';
		try {
			for (const f of files) {
				let list: ImportedBrush[];
				try {
					list = await importBrushFile(f, decode);
				} catch (e) {
					note = `${f.name}: ${(e as Error).message}`;
					continue;
				}
				for (const b of list) {
					const tip = b.tip ? saveTexture({ name: b.name, kind: 'tip', ...b.tip }) : null;
					const grain = b.grain ? saveTexture({ name: b.name, kind: 'grain', ...b.grain }) : null;
					const brush = saveCustomBrush({
						name: b.name,
						base: 'deevad-basic-digital-brush',
						tweaks: b.myb ? {} : { hardness: 1 },
						source: b.myb
					});
					if (tip || grain)
						setBrushTexture(brush.id, {
							...NO_TEXTURE,
							tip: tip?.id ?? null,
							grain: grain?.id ?? null,
							grainStrength: grain ? 0.8 : NO_TEXTURE.grainStrength
						});
					first ||= brush.id;
					made++;
				}
			}
			if (made) {
				note = `Added ${made} brush${made === 1 ? '' : 'es'} to My brushes.`;
				onpick?.(first);
			}
		} finally {
			busy = false;
		}
	}

	/** Any picture as a new grain or tip. */
	async function addPicture(files: FileList | null, kind: 'grain' | 'tip') {
		const f = files?.[0];
		if (!f) return;
		try {
			const [b] = await importBrushFile(
				new File([f], f.name.replace(/\.[^.]+$/, '') + '.png', { type: f.type }),
				decode
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

<section class="tex" aria-label="Brush texture">
	<button type="button" class="head" aria-expanded={open} onclick={() => (open = !open)}>
		<b>Texture &amp; shape</b>
		<span>{textureOn(tex) ? 'On' : 'Off'}</span>
	</button>
	{#if open}
		<div class="body">
			<h4>Grain <small>(paper texture: paint skips its low spots)</small></h4>
			<div class="pics" role="radiogroup" aria-label="Grain">
				<button
					type="button"
					class="pic none"
					role="radio"
					aria-checked={!tex.grain}
					onclick={() => set({ grain: null })}
				>
					None
				</button>
				{#each grains as t (t.id)}
					<button
						type="button"
						class="pic"
						role="radio"
						aria-checked={tex.grain === t.id}
						aria-label={t.name}
						title={t.name}
						onclick={() => set({ grain: t.id })}
					>
						<img src={thumbOf(t)} alt="" />
					</button>
				{/each}
				<label class="pic add" title="Use a picture as grain">
					+<input
						type="file"
						accept="image/*"
						onchange={(e) => addPicture(e.currentTarget.files, 'grain')}
					/>
				</label>
			</div>
			{#if tex.grain}
				<label class="slider">
					<span>Strength <b>{Math.round(tex.grainStrength * 100)}%</b></span>
					<input
						type="range"
						min="0"
						max="1"
						step="0.05"
						value={tex.grainStrength}
						oninput={(e) => set({ grainStrength: +e.currentTarget.value })}
					/>
				</label>
				<label class="slider">
					<span>Scale <b>{tex.grainScale.toFixed(1)}×</b></span>
					<input
						type="range"
						min="0.3"
						max="4"
						step="0.1"
						value={tex.grainScale}
						oninput={(e) => set({ grainScale: +e.currentTarget.value })}
					/>
				</label>
			{/if}

			<h4>Shape <small>(the brush tip)</small></h4>
			<div class="pics" role="radiogroup" aria-label="Shape">
				<button
					type="button"
					class="pic none"
					role="radio"
					aria-checked={!tex.tip}
					onclick={() => set({ tip: null })}
				>
					Round
				</button>
				{#each tips as t (t.id)}
					<button
						type="button"
						class="pic"
						role="radio"
						aria-checked={tex.tip === t.id}
						aria-label={t.name}
						title={t.name}
						onclick={() => set({ tip: t.id })}
					>
						<img src={thumbOf(t)} alt="" />
					</button>
				{/each}
				<label class="pic add" title="Use a picture as the brush shape (dark = paint)">
					+<input
						type="file"
						accept="image/*"
						onchange={(e) => addPicture(e.currentTarget.files, 'tip')}
					/>
				</label>
			</div>

			<h4>Dual brush <small>(a second shape: paint only where both meet)</small></h4>
			<div class="pics" role="radiogroup" aria-label="Dual brush">
				<button
					type="button"
					class="pic none"
					role="radio"
					aria-checked={!tex.dual}
					onclick={() => set({ dual: null })}
				>
					None
				</button>
				{#each tips as t (t.id)}
					<button
						type="button"
						class="pic"
						role="radio"
						aria-checked={tex.dual === t.id}
						aria-label="Dual: {t.name}"
						title={t.name}
						onclick={() => set({ dual: t.id })}
					>
						<img src={thumbOf(t)} alt="" />
					</button>
				{/each}
			</div>
			{#if tex.dual}
				{#each [['dualSize', 'Size', 0.1, 1.5, 0.05], ['dualScatter', 'Scatter', 0, 1.5, 0.05], ['dualCount', 'Count', 1, 8, 1]] as [k, label, min, max, step] (k)}
					{@const key = k as 'dualSize' | 'dualScatter' | 'dualCount'}
					<label class="slider">
						<span>
							{label}
							<b>{key === 'dualCount' ? tex[key] : `${Math.round(tex[key] * 100)}%`}</b>
						</span>
						<input
							type="range"
							min={min as number}
							max={max as number}
							step={step as number}
							value={tex[key]}
							oninput={(e) => set({ [key]: +e.currentTarget.value })}
						/>
					</label>
				{/each}
			{/if}

			<h4>Import brushes</h4>
			<label class="import">
				{busy ? 'Reading…' : 'Choose brush files'}
				<input
					type="file"
					multiple
					accept=".myb,.abr,.brush,.brushset,image/*"
					disabled={busy}
					onchange={(e) => importFiles(e.currentTarget.files)}
				/>
			</label>
			<p class="small">
				MyPaint (.myb), Photoshop (.abr) and Procreate (.brush, .brushset). Photoshop and Procreate
				brushes bring their shape and grain; their other settings start from defaults.
			</p>
			{#if note}<p class="note" role="status">{note}</p>{/if}
			{#if textures.some((t) => t.id.startsWith('img-'))}
				<details>
					<summary>Your pictures</summary>
					<div class="pics">
						{#each textures.filter((t) => t.id.startsWith('img-')) as t (t.id)}
							<button
								type="button"
								class="pic"
								title="Delete {t.name}"
								aria-label="Delete picture {t.name}"
								onclick={() => deleteTexture(t.id)}
							>
								<img src={thumbOf(t)} alt="" /><span class="x" aria-hidden="true">×</span>
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
	h4 {
		margin: 12px 0 6px;
		font-size: 14px;
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
		border: 2px solid var(--pp-line, #e3e3e8);
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
	.pic input,
	.import input {
		position: absolute;
		opacity: 0;
		width: 1px;
		height: 1px;
	}
	.add {
		font-size: 22px;
	}
	.x {
		position: absolute;
		top: 0;
		right: 3px;
		color: #c00;
		font-size: 16px;
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
	.import {
		position: relative;
		display: inline-flex;
		align-items: center;
		min-height: 44px;
		padding: 0 16px;
		border-radius: 12px;
		background: var(--pp-surface-2, #f1f1f4);
		font-weight: 600;
		cursor: pointer;
	}
	.small,
	.note {
		margin: 6px 0;
		font-size: 13px;
		color: var(--pp-muted, #6b6b76);
	}
</style>
