<script lang="ts" module>
	import type { LayerSettings } from './PaintCanvas.svelte';

	/** What the panel needs from the canvas (PaintCanvas's exported functions). */
	export interface LayerControls {
		selectLayer(id: number): void;
		addLayer(name?: string): void;
		duplicateLayer(id: number): void;
		deleteLayer(id: number): void;
		moveLayer(id: number, to: number): void;
		mergeDown(id: number): void;
		setLayer(id: number, patch: Partial<LayerSettings>): void;
		previewLayer(id: number, patch: Partial<LayerSettings>): void;
		commitLayer(): void;
		layerThumbnail(id: number, w: number, h: number): ImageData | null;
		addMask(id: number): void;
		deleteMask(id: number): void;
		applyMask(id: number): void;
		editMask(id: number, on: boolean): void;
		maskThumbnail(id: number, w: number, h: number): ImageData | null;
	}
</script>

<script lang="ts">
	import type { BlendMode, CanvasStatus, LayerInfo } from './PaintCanvas.svelte';

	let {
		status,
		controls,
		aspect = 4 / 3
	}: { status: CanvasStatus; controls: LayerControls | undefined; aspect?: number } = $props();

	const BLENDS: { id: BlendMode; label: string }[] = [
		{ id: 'normal', label: 'Normal' },
		{ id: 'multiply', label: 'Multiply' },
		{ id: 'screen', label: 'Screen' },
		{ id: 'overlay', label: 'Overlay' },
		{ id: 'darken', label: 'Darken' },
		{ id: 'lighten', label: 'Lighten' },
		{ id: 'add', label: 'Add (glow)' }
	];
	const MAX_LAYERS = 32;
	const TH = 36;
	const TW = $derived(Math.round(TH * aspect));

	/** top → bottom, as layer panels show them */
	const rows = $derived([...status.layers].reverse());
	const active = $derived(status.layers.find((l) => l.id === status.active) ?? null);
	const activeIndex = $derived(status.layers.findIndex((l) => l.id === status.active));

	let renaming = $state<number | null>(null);
	let dragging = $state<number | null>(null);
	let dropAt = $state<number | null>(null);
	/** preview canvases by layer id (not reactive state: written by the action below) */
	const thumbs: Record<number, HTMLCanvasElement> = {};
	const maskThumbs: Record<number, HTMLCanvasElement> = {};

	// redraw thumbnails after each change (one frame later, batched)
	let raf = 0;
	$effect(() => {
		void status.version;
		void TW;
		cancelAnimationFrame(raf);
		raf = requestAnimationFrame(() => {
			for (const l of status.layers) {
				const c = thumbs[l.id];
				const img = controls?.layerThumbnail(l.id, TW, TH);
				if (c && img) c.getContext('2d')?.putImageData(img, 0, 0);
				const mc = maskThumbs[l.id];
				const mimg = l.mask ? controls?.maskThumbnail(l.id, TW, TH) : null;
				if (mc && mimg) mc.getContext('2d')?.putImageData(mimg, 0, 0);
			}
		});
		return () => cancelAnimationFrame(raf);
	});

	function thumb(node: HTMLCanvasElement, id: number) {
		thumbs[id] = node;
		return { destroy: () => delete thumbs[id] };
	}
	function maskThumb(node: HTMLCanvasElement, id: number) {
		maskThumbs[id] = node;
		return { destroy: () => delete maskThumbs[id] };
	}

	function finishRename(l: LayerInfo, value: string) {
		renaming = null;
		const name = value.trim();
		if (name && name !== l.name) controls?.setLayer(l.id, { name });
	}

	/** rows are top → bottom; the engine counts from the bottom */
	const engineIndex = (rowIndex: number) => status.layers.length - 1 - rowIndex;

	function drop(rowIndex: number) {
		if (dragging !== null) controls?.moveLayer(dragging, engineIndex(rowIndex));
		dragging = null;
		dropAt = null;
	}
</script>

<div class="layers">
	<div class="actions" role="toolbar" aria-label="Layer actions">
		<button
			type="button"
			aria-label="New layer"
			title="New layer"
			disabled={status.layers.length >= MAX_LAYERS}
			onclick={() => controls?.addLayer()}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Duplicate layer"
			title="Duplicate layer"
			disabled={!active || status.layers.length >= MAX_LAYERS}
			onclick={() => active && controls?.duplicateLayer(active.id)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 4H6a2 2 0 0 0-2 2v10" />
			</svg>
		</button>
		<button
			type="button"
			aria-label="Merge down"
			title="Merge into the layer below"
			disabled={activeIndex <= 0}
			onclick={() => active && controls?.mergeDown(active.id)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v12M6 10l6 6 6-6M5 20h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Move layer up"
			title="Move up"
			disabled={!active || activeIndex >= status.layers.length - 1}
			onclick={() => active && controls?.moveLayer(active.id, activeIndex + 1)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
		</button>
		<button
			type="button"
			aria-label="Move layer down"
			title="Move down"
			disabled={!active || activeIndex <= 0}
			onclick={() => active && controls?.moveLayer(active.id, activeIndex - 1)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6" /></svg>
		</button>
		<button
			type="button"
			class="danger"
			aria-label="Delete layer"
			title="Delete layer"
			disabled={status.layers.length <= 1}
			onclick={() => active && controls?.deleteLayer(active.id)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="M4 7h16M6 7l1 13h10l1-13M9 7V4h6v3" />
			</svg>
		</button>
	</div>

	{#if active}
		<div class="settings">
			<label class="field">
				<span>Blend</span>
				<select
					value={active.blend}
					onchange={(e) =>
						controls?.setLayer(active.id, { blend: e.currentTarget.value as BlendMode })}
				>
					{#each BLENDS as b (b.id)}<option value={b.id}>{b.label}</option>{/each}
				</select>
			</label>
			<label class="field grow">
				<span>Opacity <b>{Math.round(active.opacity * 100)}%</b></span>
				<input
					type="range"
					min="0"
					max="100"
					value={Math.round(active.opacity * 100)}
					oninput={(e) =>
						controls?.previewLayer(active.id, { opacity: +e.currentTarget.value / 100 })}
					onchange={() => controls?.commitLayer()}
				/>
			</label>
			<div class="toggles">
				<button
					type="button"
					class="chip"
					aria-pressed={active.locked}
					title="Lock: no painting on this layer"
					onclick={() => controls?.setLayer(active.id, { locked: !active.locked })}
				>
					Lock
				</button>
				<button
					type="button"
					class="chip"
					aria-pressed={active.alphaLock}
					title="Alpha lock: paint only where this layer already has paint"
					onclick={() => controls?.setLayer(active.id, { alphaLock: !active.alphaLock })}
				>
					Alpha lock
				</button>
				<button
					type="button"
					class="chip"
					aria-pressed={active.clip}
					disabled={status.layers[0]?.id === active.id}
					title="Clipping mask: only show where the layer below has paint"
					onclick={() => controls?.setLayer(active.id, { clip: !active.clip })}
				>
					Clip to below
				</button>
				{#if active.mask}
					<button
						type="button"
						class="chip"
						aria-pressed={active.editingMask}
						title="Paint on the mask: black hides, white shows"
						onclick={() => controls?.editMask(active.id, !active.editingMask)}
					>
						Edit mask
					</button>
					<button
						type="button"
						class="chip"
						title="Bake the mask into the layer"
						onclick={() => controls?.applyMask(active.id)}
					>
						Apply mask
					</button>
					<button
						type="button"
						class="chip"
						title="Remove the mask (everything shows again)"
						onclick={() => controls?.deleteMask(active.id)}
					>
						Delete mask
					</button>
				{:else}
					<button
						type="button"
						class="chip"
						title="Layer mask: hide parts of this layer without erasing them"
						onclick={() => {
							controls?.addMask(active.id);
							controls?.editMask(active.id, true);
						}}
					>
						Add mask
					</button>
				{/if}
			</div>
			{#if active.editingMask}
				<p class="mask-hint" role="status">Painting on the mask: black hides, white shows.</p>
			{/if}
		</div>
	{/if}

	<ul class="list" aria-label="Layers, top to bottom">
		{#each rows as l, i (l.id)}
			<li
				class:active={l.id === status.active}
				class:clipped={l.clip}
				class:drop={dropAt === i && dragging !== l.id}
				draggable={renaming === null}
				ondragstart={(e) => {
					dragging = l.id;
					e.dataTransfer?.setData('text/plain', String(l.id));
				}}
				ondragover={(e) => {
					e.preventDefault();
					dropAt = i;
				}}
				ondragleave={() => (dropAt = null)}
				ondrop={(e) => {
					e.preventDefault();
					drop(i);
				}}
				ondragend={() => {
					dragging = null;
					dropAt = null;
				}}
			>
				<button
					type="button"
					class="eye"
					aria-pressed={l.visible}
					aria-label="{l.visible ? 'Hide' : 'Show'} {l.name}"
					title={l.visible ? 'Hide' : 'Show'}
					onclick={() => controls?.setLayer(l.id, { visible: !l.visible })}
				>
					{#if l.visible}
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z" /><circle
								cx="12"
								cy="12"
								r="3"
							/>
						</svg>
					{:else}
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<path
								d="M3 3l18 18M10.6 5.1A9.7 9.7 0 0 1 12 5c6.4 0 10 7 10 7a17 17 0 0 1-3.2 4M6.6 6.6A17 17 0 0 0 2 12s3.6 7 10 7a9.6 9.6 0 0 0 5.4-1.6"
							/>
						</svg>
					{/if}
				</button>
				{#if l.clip}
					<span
						class="clip-mark"
						title="Clipped to the layer below"
						aria-label="Clipped to the layer below"
						role="img"
					>
						↳
					</span>
				{/if}
				<canvas
					class="thumb"
					width={TW}
					height={TH}
					style="width:{TW}px;height:{TH}px"
					use:thumb={l.id}
					aria-hidden="true"
				></canvas>
				{#if l.mask}
					<button
						type="button"
						class="mask-btn"
						class:on={l.editingMask}
						aria-pressed={l.editingMask}
						aria-label="Edit the mask of {l.name}"
						title="Mask: click to paint on it"
						onclick={() => controls?.editMask(l.id, !l.editingMask)}
					>
						<canvas
							class="thumb mask-thumb"
							width={TW}
							height={TH}
							style="width:{TW}px;height:{TH}px"
							use:maskThumb={l.id}
							aria-hidden="true"
						></canvas>
					</button>
				{/if}
				{#if renaming === l.id}
					<!-- svelte-ignore a11y_autofocus -->
					<input
						class="rename"
						value={l.name}
						maxlength="60"
						aria-label="Layer name"
						autofocus
						onblur={(e) => finishRename(l, e.currentTarget.value)}
						onkeydown={(e) => {
							if (e.key === 'Enter') finishRename(l, e.currentTarget.value);
							if (e.key === 'Escape') renaming = null;
							e.stopPropagation();
						}}
					/>
				{:else}
					<button
						type="button"
						class="name"
						aria-current={l.id === status.active ? 'true' : undefined}
						title="Select · double-click to rename"
						onclick={() =>
							l.editingMask ? controls?.editMask(l.id, false) : controls?.selectLayer(l.id)}
						ondblclick={() => (renaming = l.id)}
						onkeydown={(e) => {
							if (e.key === 'F2') renaming = l.id;
						}}
					>
						<span class="label">{l.name}</span>
						<span class="meta">
							{#if l.blend !== 'normal'}{BLENDS.find((b) => b.id === l.blend)?.label}{/if}
							{#if l.opacity < 1}{Math.round(l.opacity * 100)}%{/if}
						</span>
					</button>
				{/if}
				{#if l.locked || l.alphaLock}
					<span class="badge" title={l.locked ? 'Locked' : 'Alpha locked'}>
						<svg viewBox="0 0 24 24" aria-label={l.locked ? 'Locked' : 'Alpha locked'} role="img">
							<rect x="5" y="11" width="14" height="9" rx="2" /><path
								d="M8 11V8a4 4 0 0 1 8 0v3"
							/>{#if l.alphaLock && !l.locked}<circle cx="12" cy="15.5" r="1.5" />{/if}
						</svg>
					</span>
				{/if}
				{#if l.id === status.active && renaming !== l.id}
					<button
						type="button"
						class="mini"
						aria-label="Rename {l.name}"
						title="Rename (F2)"
						onclick={() => (renaming = l.id)}
					>
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
						</svg>
					</button>
				{/if}
			</li>
		{/each}
		<li class="paper" aria-label="Paper (white background)">
			<span class="eye-space"></span>
			<span class="thumb paper-thumb" style="width:{TW}px;height:{TH}px"></span>
			<span class="label muted">Paper</span>
		</li>
	</ul>
</div>

<style>
	.layers {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	svg {
		width: 18px;
		height: 18px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.actions button,
	.eye,
	.mini {
		display: grid;
		place-items: center;
		width: 40px;
		height: 40px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.actions button:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.actions .danger:not(:disabled):hover {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.settings {
		display: flex;
		flex-wrap: wrap;
		gap: 10px 14px;
		align-items: end;
		padding: 12px;
		border-radius: 16px;
		background: var(--pp-surface-sunk);
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.field.grow {
		flex: 1 1 140px;
	}
	.field input {
		accent-color: var(--pp-accent);
	}
	select {
		min-height: 36px;
		padding: 4px 8px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 500 14px var(--pp-font-body);
	}
	.toggles {
		display: flex;
		gap: 6px;
	}
	.chip {
		min-height: 36px;
		padding: 6px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.chip[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
		max-height: 340px;
		overflow-y: auto;
	}
	li {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 4px;
		border-radius: 14px;
		border: 2px solid transparent;
	}
	li.active {
		background: var(--pp-surface-sunk);
		border-color: var(--pp-ink);
	}
	li.drop {
		border-top-color: var(--pp-accent);
	}
	.eye {
		flex: none;
		background: transparent;
	}
	.eye[aria-pressed='false'] {
		color: var(--pp-muted);
	}
	.thumb {
		flex: none;
		border-radius: 6px;
		border: 1px solid var(--pp-track);
		/* checkerboard = transparent */
		background: repeating-conic-gradient(#e9e6de 0 25%, #fff 0 50%) 0 0 / 10px 10px;
	}
	.mask-btn {
		flex: none;
		display: grid;
		padding: 0;
		border: 0;
		border-radius: 8px;
		background: transparent;
		cursor: pointer;
	}
	.mask-btn.on {
		outline: 2px solid var(--pp-accent-strong);
		outline-offset: 1px;
	}
	.mask-thumb {
		background: #fff;
	}
	.mask-hint {
		margin: 6px 0 0;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.paper-thumb {
		background: #fff;
	}
	.name {
		flex: 1;
		min-width: 0;
		min-height: 40px;
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		justify-content: center;
		padding: 0 4px;
		border: 0;
		background: none;
		color: var(--pp-ink);
		text-align: left;
		cursor: pointer;
	}
	.label {
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 600 14px var(--pp-font-body);
	}
	.meta {
		font-size: 12px;
		color: var(--pp-muted);
	}
	.meta:empty {
		display: none;
	}
	.rename {
		flex: 1;
		min-width: 0;
		min-height: 36px;
		padding: 4px 8px;
		border: 2px solid var(--pp-accent);
		border-radius: 10px;
		font: 600 14px var(--pp-font-body);
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	li.clipped {
		padding-left: 18px;
	}
	.clip-mark {
		flex: none;
		color: var(--pp-accent-strong);
		font-weight: 800;
	}
	.badge {
		flex: none;
		display: grid;
		place-items: center;
		color: var(--pp-muted);
	}
	.mini {
		flex: none;
		width: 32px;
		height: 32px;
		background: transparent;
	}
	.eye-space {
		flex: none;
		width: 40px;
	}
	.muted {
		color: var(--pp-muted);
	}
</style>
