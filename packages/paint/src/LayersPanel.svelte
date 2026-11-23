<script lang="ts" module>
	import type { GroupSettings, ImportResult, LayerSettings } from './PaintCanvas.svelte';

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
		moveLayerTo(id: number, to: number, group: number): void;
		importImage(file: Blob, name?: string): Promise<ImportResult>;
		addGroup(layer: number): number;
		setGroup(id: number, patch: Partial<GroupSettings>): void;
		previewGroup(id: number, patch: Partial<GroupSettings>): void;
		commitGroup(): void;
		ungroup(id: number): void;
		deleteGroup(id: number): void;
		mergeGroup(id: number): void;
		moveGroup(id: number, up: boolean): void;
		/** drawing assist on a layer (strokes snap to the guides) */
		setLayerAssist?(id: number, on: boolean): void;
		/** select what's on a layer; `op` 0 replace, 1 add, 2 subtract, 3 intersect */
		selectLayerContents?(id: number, op?: number): void;
		/** turn a text or line layer into a plain one */
		rasterizeLayer?(id: number): void;
		/** a new line (vector) layer above the current one */
		addLineLayer?(name?: string): void;
	}
</script>

<script lang="ts">
	import type { BlendMode, CanvasStatus, GroupInfo, LayerInfo } from './PaintCanvas.svelte';
	import { NO_STYLE, type LayerStyle, type StrokePosition } from './layer-style';

	let {
		status,
		controls,
		aspect = 4 / 3,
		onadjustment,
		ontext,
		lineLayers = false
	}: {
		status: CanvasStatus;
		controls: LayerControls | undefined;
		aspect?: number;
		/** open the Adjust panel: on the given adjustment layer, or (0) to make a new one */
		onadjustment?: (id: number) => void;
		/** change a text layer's text (the text tool on it) */
		ontext?: (id: number) => void;
		/** offer line (vector) layers (the screen has the Select line tool) */
		lineLayers?: boolean;
	} = $props();

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

	// ---- layer style: changes preview live, and each slider drag is one undo step ----
	const look = $derived(active?.style ?? NO_STYLE);
	/** the active layer's style with one effect changed */
	function restyle<K extends keyof LayerStyle>(k: K, patch: Partial<LayerStyle[K]>): LayerStyle {
		return { ...look, [k]: { ...look[k], ...patch } };
	}
	function setStyle<K extends keyof LayerStyle>(k: K, patch: Partial<LayerStyle[K]>) {
		if (active) controls?.setLayer(active.id, { style: restyle(k, patch) });
	}
	function previewStyle<K extends keyof LayerStyle>(k: K, patch: Partial<LayerStyle[K]>) {
		if (active) controls?.previewLayer(active.id, { style: restyle(k, patch) });
	}
	const POSITIONS: { id: StrokePosition; label: string }[] = [
		{ id: 'outside', label: 'Outside' },
		{ id: 'centre', label: 'Centre' },
		{ id: 'inside', label: 'Inside' }
	];

	let renaming = $state<number | null>(null);
	/** a group picked in the list (its settings show instead of the layer's) */
	let selectedGroup = $state<number | null>(null);
	let renamingGroup = $state<number | null>(null);
	let collapsed = $state<number[]>([]);
	const groupById = $derived(new Map(status.groups.map((g) => [g.id, g])));
	const group = $derived(selectedGroup !== null ? (groupById.get(selectedGroup) ?? null) : null);
	const isCollapsed = (gid: number) => collapsed.includes(gid);
	function toggleCollapsed(gid: number) {
		collapsed = isCollapsed(gid) ? collapsed.filter((x) => x !== gid) : [...collapsed, gid];
	}
	function finishGroupRename(g: GroupInfo, value: string) {
		renamingGroup = null;
		const name = value.trim();
		if (name && name !== g.name) controls?.setGroup(g.id, { name });
	}
	/** move the active layer one step; it takes the group of the layer it passes */
	function step(up: boolean) {
		if (!active) return;
		const to = activeIndex + (up ? 1 : -1);
		const passed = status.layers[to];
		if (passed) controls?.moveLayerTo(active.id, to, passed.group);
	}
	/** take the active layer out of its group (it lands just above the group) */
	function leaveGroup() {
		if (!active?.group) return;
		let top = activeIndex;
		while (status.layers[top + 1]?.group === active.group) top++;
		controls?.moveLayerTo(active.id, top, 0);
	}
	let picker: HTMLInputElement;
	let importMsg = $state('');
	function importMessage(r: ImportResult): string {
		return {
			ok: '',
			'too-many': 'You’ve reached the layer limit (32).',
			unreadable: 'That file couldn’t be opened as a picture.',
			busy: 'Finish what you’re doing first, then import again.',
			'too-big': 'That picture is too big (30 MB at most).'
		}[r];
	}
	async function importPicked() {
		const file = picker.files?.[0];
		picker.value = '';
		if (!file || !controls) return;
		importMsg = importMessage(await controls.importImage(file, file.name));
	}

	function pickLayer(id: number) {
		selectedGroup = null;
		controls?.selectLayer(id);
	}
	let dragging = $state<number | null>(null);
	let dropAt = $state<number | null>(null);
	/** preview canvases by layer id (not reactive state: written by the action below) */
	/**
	 * Ctrl/⌘+click a layer: select what's on it (as in Photoshop). With Shift it adds
	 * to the selection, with Alt it takes away, with both it keeps what's in both.
	 */
	function contentsClick(e: MouseEvent, id: number): boolean {
		if (!(e.ctrlKey || e.metaKey) || !controls?.selectLayerContents) return false;
		e.preventDefault();
		const op = e.shiftKey && e.altKey ? 3 : e.shiftKey ? 1 : e.altKey ? 2 : 0;
		controls.selectLayerContents(id, op);
		return true;
	}

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
		// dropped next to a layer: it joins that layer's group
		if (dragging !== null)
			controls?.moveLayerTo(dragging, engineIndex(rowIndex), rows[rowIndex]?.group ?? 0);
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
		{#if lineLayers && controls?.addLineLayer}
			<button
				type="button"
				aria-label="New line layer"
				title="New line layer: every line stays editable (select, move, reshape, recolour, erase whole lines), as Clip Studio’s vector layers"
				disabled={status.layers.length >= MAX_LAYERS}
				onclick={() => controls?.addLineLayer?.()}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<path d="M4 17c3-7 7-10 13-10" /><circle cx="4" cy="17" r="1.6" /><circle
						cx="17"
						cy="7"
						r="1.6"
					/><path d="M18 14v6M15 17h6" />
				</svg>
			</button>
		{/if}
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
			onclick={() => step(true)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
		</button>
		<button
			type="button"
			aria-label="Move layer down"
			title="Move down"
			disabled={!active || activeIndex <= 0}
			onclick={() => step(false)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M6 13l6 6 6-6" /></svg>
		</button>
		{#if onadjustment}
			<button
				type="button"
				aria-label="New adjustment layer"
				title="New adjustment layer: curves, levels, colour or a gradient map over everything below, changeable later"
				disabled={status.layers.length >= MAX_LAYERS}
				onclick={() => onadjustment?.(0)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true">
					<circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 1 0 16Z" />
				</svg>
			</button>
		{/if}
		<button
			type="button"
			aria-label="Import picture"
			title="Import a picture as a new layer (it stays on this device)"
			disabled={status.layers.length >= MAX_LAYERS}
			onclick={() => picker.click()}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<rect x="3" y="5" width="18" height="14" rx="2" /><circle cx="9" cy="10" r="1.6" /><path
					d="M21 16l-5-5-9 8"
				/>
			</svg>
		</button>
		<input
			bind:this={picker}
			class="file"
			type="file"
			accept="image/*"
			tabindex="-1"
			aria-hidden="true"
			onchange={importPicked}
		/>
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

	{#if importMsg}<p class="import-msg" role="alert">{importMsg}</p>{/if}

	{#if group}
		<div class="settings" role="group" aria-label="Group {group.name}">
			<p class="group-title">Group: <b>{group.name}</b></p>
			<label class="field">
				<span>Group opacity <b>{Math.round(group.opacity * 100)}%</b></span>
				<input
					type="range"
					min="0"
					max="100"
					value={Math.round(group.opacity * 100)}
					aria-label="Group opacity"
					oninput={(e) =>
						controls?.previewGroup(group.id, { opacity: +e.currentTarget.value / 100 })}
					onchange={() => controls?.commitGroup()}
				/>
			</label>
			<div class="toggles">
				<button type="button" class="chip" onclick={() => controls?.moveGroup(group.id, true)}>
					Move up
				</button>
				<button type="button" class="chip" onclick={() => controls?.moveGroup(group.id, false)}>
					Move down
				</button>
				<button
					type="button"
					class="chip"
					title="Keep the layers, remove the group"
					onclick={() => {
						controls?.ungroup(group.id);
						selectedGroup = null;
					}}
				>
					Ungroup
				</button>
				<button
					type="button"
					class="chip"
					title="Flatten the group into one layer"
					onclick={() => {
						controls?.mergeGroup(group.id);
						selectedGroup = null;
					}}
				>
					Merge group
				</button>
				<button
					type="button"
					class="chip"
					title="Delete the group and its layers"
					disabled={status.layers.every((l) => l.group === group.id)}
					onclick={() => {
						controls?.deleteGroup(group.id);
						selectedGroup = null;
					}}
				>
					Delete group
				</button>
			</div>
		</div>
	{:else if active}
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
				{#if controls?.setLayerAssist}
					<button
						type="button"
						class="chip"
						aria-pressed={!!active.assist}
						title="Drawing assist: strokes on this layer snap to the grid, isometric or perspective guide"
						onclick={() => controls?.setLayerAssist?.(active.id, !active.assist)}
					>
						Drawing assist
					</button>
				{/if}
				{#if controls?.selectLayerContents}
					<button
						type="button"
						class="chip"
						title="Select what's on this layer (or Ctrl+click its picture)"
						onclick={() => controls?.selectLayerContents?.(active.id)}
					>
						Select contents
					</button>
				{/if}
				{#if active.group}
					<button
						type="button"
						class="chip"
						title="Take this layer out of its group"
						onclick={leaveGroup}
					>
						Leave group
					</button>
				{:else}
					<button
						type="button"
						class="chip"
						title="Put this layer in a new group"
						onclick={() => {
							const gid = controls?.addGroup(active.id);
							if (gid) selectedGroup = null;
						}}
					>
						New group
					</button>
				{/if}
				{#if active.lines && controls?.rasterizeLayer}
					<button
						type="button"
						class="chip"
						title="Make this line layer a plain layer of pixels (its lines can’t be selected and changed after that; a plain layer can’t become a line layer)"
						onclick={() => controls?.rasterizeLayer?.(active.id)}
					>
						Convert to pixels
					</button>
				{/if}
				{#if active.text && controls?.rasterizeLayer}
					<button
						type="button"
						class="chip"
						title="Make this text layer a plain layer to paint on (its text can’t be changed after that)"
						onclick={() => controls?.rasterizeLayer?.(active.id)}
					>
						Rasterize
					</button>
				{/if}
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
			{#if !active.adjust}
				<details class="style" open={look.stroke.on || look.shadow.on || look.glow.on}>
					<summary>Layer style</summary>
					<div class="effect">
						<button
							type="button"
							class="chip"
							aria-pressed={look.stroke.on}
							title="Outline the paint on this layer"
							onclick={() => setStyle('stroke', { on: !look.stroke.on })}
						>
							Stroke
						</button>
						{#if look.stroke.on}
							<label class="field">
								<span>Colour</span>
								<input
									type="color"
									value={look.stroke.colour}
									aria-label="Stroke colour"
									oninput={(e) => previewStyle('stroke', { colour: e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field">
								<span>Position</span>
								<select
									value={look.stroke.position}
									onchange={(e) =>
										setStyle('stroke', { position: e.currentTarget.value as StrokePosition })}
								>
									{#each POSITIONS as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
								</select>
							</label>
							<label class="field grow">
								<span>Size <b>{look.stroke.size} px</b></span>
								<input
									type="range"
									min="1"
									max="100"
									value={look.stroke.size}
									oninput={(e) => previewStyle('stroke', { size: +e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Opacity <b>{Math.round(look.stroke.opacity * 100)}%</b></span>
								<input
									type="range"
									min="0"
									max="100"
									value={Math.round(look.stroke.opacity * 100)}
									oninput={(e) => previewStyle('stroke', { opacity: +e.currentTarget.value / 100 })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
						{/if}
					</div>
					<div class="effect">
						<button
							type="button"
							class="chip"
							aria-pressed={look.shadow.on}
							title="A soft shadow behind the paint"
							onclick={() => setStyle('shadow', { on: !look.shadow.on })}
						>
							Drop shadow
						</button>
						{#if look.shadow.on}
							<label class="field">
								<span>Colour</span>
								<input
									type="color"
									value={look.shadow.colour}
									aria-label="Shadow colour"
									oninput={(e) => previewStyle('shadow', { colour: e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Opacity <b>{Math.round(look.shadow.opacity * 100)}%</b></span>
								<input
									type="range"
									min="0"
									max="100"
									value={Math.round(look.shadow.opacity * 100)}
									oninput={(e) => previewStyle('shadow', { opacity: +e.currentTarget.value / 100 })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Angle <b>{Math.round(look.shadow.angle)}°</b></span>
								<input
									type="range"
									min="0"
									max="359"
									value={Math.round(look.shadow.angle)}
									title="Where the light comes from"
									oninput={(e) => previewStyle('shadow', { angle: +e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Distance <b>{look.shadow.distance} px</b></span>
								<input
									type="range"
									min="0"
									max="200"
									value={look.shadow.distance}
									oninput={(e) => previewStyle('shadow', { distance: +e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Blur <b>{look.shadow.blur} px</b></span>
								<input
									type="range"
									min="0"
									max="100"
									value={look.shadow.blur}
									oninput={(e) => previewStyle('shadow', { blur: +e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
						{/if}
					</div>
					<div class="effect">
						<button
							type="button"
							class="chip"
							aria-pressed={look.glow.on}
							title="A soft glow around the paint"
							onclick={() => setStyle('glow', { on: !look.glow.on })}
						>
							Outer glow
						</button>
						{#if look.glow.on}
							<label class="field">
								<span>Colour</span>
								<input
									type="color"
									value={look.glow.colour}
									aria-label="Glow colour"
									oninput={(e) => previewStyle('glow', { colour: e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Size <b>{look.glow.size} px</b></span>
								<input
									type="range"
									min="1"
									max="100"
									value={look.glow.size}
									oninput={(e) => previewStyle('glow', { size: +e.currentTarget.value })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
							<label class="field grow">
								<span>Opacity <b>{Math.round(look.glow.opacity * 100)}%</b></span>
								<input
									type="range"
									min="0"
									max="100"
									value={Math.round(look.glow.opacity * 100)}
									oninput={(e) => previewStyle('glow', { opacity: +e.currentTarget.value / 100 })}
									onchange={() => controls?.commitLayer()}
								/>
							</label>
						{/if}
					</div>
				</details>
			{/if}
		</div>
	{/if}

	<ul class="list" aria-label="Layers, top to bottom">
		{#each rows as l, i (l.id)}
			{@const g = l.group ? groupById.get(l.group) : undefined}
			{#if g && rows[i - 1]?.group !== g.id}
				<li class="group-row" class:active={selectedGroup === g.id}>
					<button
						type="button"
						class="eye"
						aria-pressed={g.visible}
						aria-label="{g.visible ? 'Hide' : 'Show'} group {g.name}"
						title={g.visible ? 'Hide group' : 'Show group'}
						onclick={() => controls?.setGroup(g.id, { visible: !g.visible })}
					>
						<svg viewBox="0 0 24 24" aria-hidden="true">
							{#if g.visible}
								<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z" /><circle
									cx="12"
									cy="12"
									r="3"
								/>
							{:else}
								<path d="M3 3l18 18M6.6 6.6A17 17 0 0 0 2 12s3.6 7 10 7a9.6 9.6 0 0 0 5.4-1.6" />
							{/if}
						</svg>
					</button>
					<button
						type="button"
						class="fold"
						aria-expanded={!isCollapsed(g.id)}
						aria-label="{isCollapsed(g.id) ? 'Expand' : 'Collapse'} group {g.name}"
						onclick={() => toggleCollapsed(g.id)}
					>
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<path d={isCollapsed(g.id) ? 'M9 6l6 6-6 6' : 'M6 9l6 6 6-6'} />
						</svg>
					</button>
					{#if renamingGroup === g.id}
						<!-- svelte-ignore a11y_autofocus -->
						<input
							class="rename"
							value={g.name}
							maxlength="60"
							aria-label="Group name"
							autofocus
							onblur={(e) => finishGroupRename(g, e.currentTarget.value)}
							onkeydown={(e) => {
								if (e.key === 'Enter') finishGroupRename(g, e.currentTarget.value);
								if (e.key === 'Escape') renamingGroup = null;
								e.stopPropagation();
							}}
						/>
					{:else}
						<button
							type="button"
							class="name"
							aria-current={selectedGroup === g.id ? 'true' : undefined}
							title="Select group · double-click to rename"
							onclick={() => (selectedGroup = selectedGroup === g.id ? null : g.id)}
							ondblclick={() => (renamingGroup = g.id)}
						>
							<span class="label">
								<svg class="folder" viewBox="0 0 24 24" aria-hidden="true">
									<path
										d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
									/>
								</svg>
								{g.name}
							</span>
							<span class="meta">
								{#if g.opacity < 1}{Math.round(g.opacity * 100)}%{/if}
							</span>
						</button>
					{/if}
				</li>
			{/if}
			{#if !(g && isCollapsed(g.id))}
				<li
					class:active={l.id === status.active && selectedGroup === null}
					class:member={!!g}
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
					{#if l.adjust}
						<button
							type="button"
							class="thumb adjust-thumb"
							style="width:{TW}px;height:{TH}px"
							aria-label="Change the adjustment {l.name}"
							title="Adjustment layer: changes everything below it. Click to change it."
							onclick={() => onadjustment?.(l.id)}
						>
							<svg viewBox="0 0 24 24" aria-hidden="true">
								<circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 1 0 16Z" />
							</svg>
						</button>
					{:else}
						<!-- (Ctrl+click: select its contents; the name button does the same from the keyboard) -->
						<canvas
							class="thumb"
							width={TW}
							height={TH}
							style="width:{TW}px;height:{TH}px"
							use:thumb={l.id}
							aria-hidden="true"
							title="Ctrl+click: select what's on this layer"
							onclick={(e) => contentsClick(e, l.id) || pickLayer(l.id)}
						></canvas>
					{/if}
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
							onclick={(e) =>
								contentsClick(e, l.id) ||
								(l.editingMask ? controls?.editMask(l.id, false) : pickLayer(l.id))}
							ondblclick={() => (renaming = l.id)}
							onkeydown={(e) => {
								if (e.key === 'F2') renaming = l.id;
							}}
						>
							<span class="label">{l.name}</span>
							<span class="meta">
								{#if l.blend !== 'normal'}{BLENDS.find((b) => b.id === l.blend)?.label}{/if}
								{#if l.opacity < 1}{Math.round(l.opacity * 100)}%{/if}
								{#if l.assist}Assisted{/if}
							</span>
						</button>
					{/if}
					{#if l.lines}
						<span class="badge line-badge" title="Line layer: its lines stay editable">
							<svg viewBox="0 0 24 24" aria-label="Line layer" role="img">
								<path d="M4 18c3-8 8-12 16-12" /><circle cx="4" cy="18" r="1.8" /><circle
									cx="20"
									cy="6"
									r="1.8"
								/>
							</svg>
						</span>
					{/if}
					{#if l.text}
						<button
							type="button"
							class="mini text-badge"
							aria-label="Change the text of {l.name}"
							title="Text layer: click to change the text"
							onclick={() => ontext?.(l.id)}
						>
							T
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
			{/if}
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
		flex-wrap: wrap;
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
		color: var(--pp-on-ink);
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
		/* checkerboard = transparent (the app can set its colour and size) */
		background: repeating-conic-gradient(var(--pp-checker, #e9e6de) 0 25%, #fff 0 50%) 0 0 /
			var(--pp-checker-size, 10px) var(--pp-checker-size, 10px);
	}
	.file {
		display: none;
	}
	.import-msg {
		margin: 0;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-strong);
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
	.adjust-thumb {
		display: grid;
		place-items: center;
		padding: 0;
		border: 0;
		cursor: pointer;
	}
	.adjust-thumb svg {
		width: 22px;
		height: 22px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2;
	}
	.adjust-thumb svg path {
		fill: currentColor;
	}
	.mask-thumb {
		background: #fff;
	}
	.style {
		flex: 1 1 100%;
	}
	.style summary {
		min-height: 32px;
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-ink-2);
		cursor: pointer;
	}
	.effect {
		display: flex;
		flex-wrap: wrap;
		gap: 8px 14px;
		align-items: end;
		padding: 8px 0;
	}
	.effect input[type='color'] {
		width: 44px;
		height: 32px;
		padding: 0;
		border: 1px solid var(--pp-track);
		border-radius: 8px;
		background: var(--pp-surface);
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
	li.member {
		margin-left: 14px;
		border-left: 2px solid var(--pp-track);
		border-radius: 0 14px 14px 0;
	}
	li.member.active {
		border-left-color: var(--pp-ink);
	}
	.group-row {
		background: var(--pp-surface-sunk);
	}
	.fold {
		flex: none;
		width: 28px;
		background: transparent;
	}
	.folder {
		width: 16px;
		height: 16px;
		vertical-align: -3px;
		margin-right: 4px;
	}
	.group-title {
		margin: 0 0 6px;
		font-size: 14px;
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
	.text-badge {
		font: 700 15px var(--pp-font-display, serif);
	}
	.eye-space {
		flex: none;
		width: 40px;
	}
	.muted {
		color: var(--pp-muted);
	}
</style>
