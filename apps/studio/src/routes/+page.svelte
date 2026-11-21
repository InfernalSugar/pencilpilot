<script lang="ts">
	import { resolve } from '$app/paths';
	/**
	 * The drawing studio, in the app: the website's studio (same layout, tools and panels,
	 * from @pencilpilot/paint) without the website's practice parts, plus projects:
	 * Ctrl+S saves (.ppstudio), Ctrl+Shift+S saves as, Ctrl+O opens.
	 */
	import Studio from '@pencilpilot/paint/Studio.svelte';
	import ToolRail from '@pencilpilot/paint/ToolRail.svelte';
	import PaintCanvas, { type CanvasStatus } from '@pencilpilot/paint/PaintCanvas.svelte';
	import LayersPanel from '@pencilpilot/paint/LayersPanel.svelte';
	import BrushPicker from '@pencilpilot/paint/BrushPicker.svelte';
	import BrushTexturePanel from '@pencilpilot/paint/BrushTexturePanel.svelte';
	import ColorPanel from '@pencilpilot/paint/ColorPanel.svelte';
	import HistoryPanel from '@pencilpilot/paint/HistoryPanel.svelte';
	import Swatches from '@pencilpilot/paint/Swatches.svelte';
	import { brushTexture, onTextures, textureOn } from '@pencilpilot/paint/brush-textures';
	import {
		brushLabel,
		DEFAULT_BRUSH,
		loadManifest,
		type BrushManifest
	} from '@pencilpilot/paint/brushes';
	import Logo from '@pencilpilot/ui/Logo.svelte';
	import { onMount, tick, untrack } from 'svelte';
	import { isTauri } from '@tauri-apps/api/core';
	import {
		askExportPath,
		askOpenPath,
		askSavePath,
		discardRecovery,
		listRecovery,
		newDocId,
		openProject,
		openRecovery,
		openVersion,
		projectName,
		saveProject,
		writeExport,
		writeRecovery,
		type ProjectDoc,
		type RecoveryItem
	} from '#lib/project.js';
	import RecoverDialog from '#lib/RecoverDialog.svelte';
	import VersionsMenu from '#lib/VersionsMenu.svelte';

	let canvas = $state<PaintCanvas>();
	let status = $state<CanvasStatus | null>(null);
	let tool = $state<string>('brush');
	let eraser = $state(false);
	let size = $state(1);
	let opacity = $state(1);
	let color = $state('#16161a');
	let brushId = $state(DEFAULT_BRUSH);
	let manifest = $state<BrushManifest | null>(null);
	let tab = $state('layers');
	let dockOpen = $state(false);
	let smoothing = $state(0.2);
	/** the canvas size (a project brings its own) */
	let canvasSize = $state({ width: 1920, height: 1080 });
	let fps = $state(24);
	/** bumps for a new, empty canvas (opening a project of another size remounts it) */
	let canvasKey = $state(0);

	// ---- the project file ----
	let path = $state<string | null>(null);
	let created = $state<string | undefined>(undefined);
	/** this document's recovery copy (a new one for each opened or new document) */
	let docId = newDocId();
	/** the canvas version when last saved or opened (anything newer is unsaved; -1: all of it) */
	let savedVersion = $state(1);
	/** the canvas version in the recovery copy */
	let recoveredVersion = 0;
	let busy = $state<'' | 'saving' | 'opening' | 'exporting'>('');
	let fileNote = $state('');
	const unsaved = $derived(
		!!status && (savedVersion === -1 || (!status.blank && status.version !== savedVersion))
	);
	/** a document being loaded, waiting for its canvas (and whether it starts unsaved) */
	let pending = $state.raw<{ doc: ProjectDoc; unsaved: boolean } | null>(null);
	/** recovery copies found at start-up */
	let recoverable = $state<RecoveryItem[]>([]);
	let exportOpen = $state(false);
	let recoverLater = $state(false);

	/** Everything the window knows, as a project document. */
	async function currentDoc(): Promise<ProjectDoc> {
		if (!canvas) throw new Error('the canvas isn’t ready');
		const snap = canvas.snapshotLayers();
		if (!snap) throw new Error('the canvas isn’t ready');
		return {
			width: canvasSize.width,
			height: canvasSize.height,
			fps,
			layers: snap.layers,
			groups: snap.groups,
			activeLayer: status?.active ?? 0,
			strokes: snap.strokes,
			extra: { brushId, color, size, opacity, smoothing },
			created,
			thumbnail: await canvas.thumbnail()
		};
	}

	async function save(as = false) {
		if (!canvas || busy) return;
		const target = as || !path ? await askSavePath(projectName(path)) : path;
		if (!target) return;
		busy = 'saving';
		fileNote = '';
		try {
			const versionAtSave = status?.version ?? 0;
			path = await saveProject(target, await currentDoc());
			created ??= new Date().toISOString();
			savedVersion = versionAtSave;
			// saved for real: the recovery copy has done its job
			await discardRecovery(docId).catch(() => {});
			recoveredVersion = 0;
		} catch (e) {
			fileNote = `Couldn’t save: ${e}`;
		} finally {
			busy = '';
		}
	}

	/** Unsaved changes would be lost: ask first. True to go ahead. */
	async function okToDrop(): Promise<boolean> {
		if (!unsaved) return true;
		const { ask } = await import('@tauri-apps/plugin-dialog');
		return ask(`“${projectName(path)}” has changes that aren’t saved. Lose them?`, {
			title: 'Unsaved changes',
			kind: 'warning',
			okLabel: 'Lose changes',
			cancelLabel: 'Keep working'
		});
	}

	/** Put a document on a fresh canvas of its size. */
	function load(doc: ProjectDoc, opts: { path: string | null; unsaved: boolean; id?: string }) {
		path = opts.path;
		created = doc.created;
		fps = doc.fps;
		docId = opts.id ?? newDocId();
		recoveredVersion = 0;
		const x = doc.extra;
		if (typeof x.brushId === 'string') brushId = x.brushId;
		if (typeof x.color === 'string') color = x.color;
		if (typeof x.size === 'number') size = x.size;
		if (typeof x.opacity === 'number') opacity = x.opacity;
		if (typeof x.smoothing === 'number') smoothing = x.smoothing;
		status = null;
		canvasSize = { width: doc.width, height: doc.height };
		canvasKey++;
		pending = { doc, unsaved: opts.unsaved };
	}

	async function run(what: 'opening', job: () => Promise<void>) {
		if (busy) return;
		busy = what;
		fileNote = '';
		try {
			await job();
			await tick();
		} catch (e) {
			fileNote = `Couldn’t open: ${e}`;
			busy = '';
		}
	}

	async function open() {
		if (busy || !(await okToDrop())) return;
		const target = await askOpenPath();
		if (!target) return;
		const old = docId;
		await run('opening', async () => {
			load(await openProject(target), { path: target, unsaved: false });
			await discardRecovery(old).catch(() => {});
		});
	}

	async function openOldVersion(id: string) {
		if (!path || busy || !(await okToDrop())) return;
		const file = path;
		await run('opening', async () =>
			load(await openVersion(file, id), { path: file, unsaved: true })
		);
	}

	async function recover(item: RecoveryItem) {
		recoverable = recoverable.filter((r) => r.id !== item.id);
		// keeps the same recovery copy, so more autosaves update it
		await run('opening', async () =>
			load(await openRecovery(item.id), { path: item.path, unsaved: true, id: item.id })
		);
	}

	async function discard(item: RecoveryItem) {
		recoverable = recoverable.filter((r) => r.id !== item.id);
		await discardRecovery(item.id).catch(() => {});
	}

	async function exportAs(kind: 'png' | 'psd') {
		if (!canvas || busy) return;
		const target = await askExportPath(projectName(path), kind);
		if (!target) return;
		busy = 'exporting';
		fileNote = '';
		try {
			let bytes: Uint8Array;
			if (kind === 'png') {
				const d = canvas.exportDrawing();
				if (!d) throw new Error('nothing to export');
				bytes = new Uint8Array(await (await fetch(d.png)).arrayBuffer());
			} else {
				const doc = canvas.exportLayered();
				if (!doc) throw new Error('nothing to export');
				const { writePsd } = await import('@pencilpilot/paint/psd');
				bytes = writePsd(doc);
			}
			await writeExport(target, bytes);
		} catch (e) {
			fileNote = `Couldn’t export: ${e}`;
		} finally {
			busy = '';
		}
	}

	// the new canvas is ready: put the document's layers on it
	$effect(() => {
		const p = pending;
		if (!p || !canvas || !status) return;
		pending = null;
		canvas.restoreLayers(p.doc.layers, p.doc.groups, p.doc.strokes);
		if (p.doc.activeLayer) canvas.selectLayer(p.doc.activeLayer);
		// opened: nothing unsaved (the canvas reported its new state as it restored);
		// recovered or an old version: all of it is unsaved
		savedVersion = p.unsaved ? -1 : untrack(() => status?.version ?? 0);
		busy = '';
	});

	// autosave: a recovery copy of unsaved work, every 20 seconds while it changes
	const AUTOSAVE_MS = 20_000;
	async function autosave() {
		if (!unsaved || busy || !status || status.version === recoveredVersion) return;
		const version = status.version;
		try {
			await writeRecovery(docId, path, await currentDoc());
			recoveredVersion = version;
		} catch {
			// a failed autosave is retried next time; the work is still on screen
		}
	}

	onMount(() => {
		if (!isTauri()) return;
		const timer = setInterval(autosave, AUTOSAVE_MS);
		listRecovery()
			.then((list) => (recoverable = list))
			.catch(() => {});
		// closing with unsaved work: ask; closing on purpose drops the recovery copy
		let stop: (() => void) | undefined;
		import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
			getCurrentWindow()
				.onCloseRequested(async (e) => {
					if (unsaved && !(await okToDrop())) return e.preventDefault();
					await discardRecovery(docId).catch(() => {});
				})
				.then((un) => (stop = un))
		);
		return () => {
			clearInterval(timer);
			stop?.();
		};
	});

	onMount(() => {
		loadManifest()
			.then((m) => (manifest = m))
			.catch(() => (manifest = null));
	});

	let texVersion = $state(0);
	$effect(() => onTextures(() => texVersion++));
	const texture = $derived.by(() => {
		void texVersion;
		const t = brushTexture(brushId);
		return textureOn(t) ? t : null;
	});

	const TABS = $derived([
		{ id: 'layers', label: 'Layers', meta: `${status?.layers.length ?? 1} layers` },
		{ id: 'brushes', label: 'Brushes', meta: manifest ? brushLabel(manifest, brushId) : '…' },
		{ id: 'colour', label: 'Colour' },
		{ id: 'history', label: 'History' }
	]);

	const TOOL_KEYS: Record<string, string> = {
		i: 'picker',
		g: 'fill',
		f: 'lasso-fill',
		u: 'measure',
		s: 'smudge',
		p: 'pen',
		v: 'transform',
		k: 'liquify',
		m: 'select-rect',
		l: 'select-lasso',
		w: 'select-wand'
	};

	function onkey(e: KeyboardEvent) {
		if ((e.target as HTMLElement).closest('input, textarea, select')) return;
		const mod = e.ctrlKey || e.metaKey;
		const k = e.key.toLowerCase();
		if (mod && k === 's') {
			e.preventDefault();
			void save(e.shiftKey);
		} else if (mod && k === 'o') {
			e.preventDefault();
			void open();
		} else if (mod && k === 'z') {
			e.preventDefault();
			if (e.shiftKey) canvas?.redo();
			else canvas?.undo();
		} else if (mod && k === 'y') {
			e.preventDefault();
			canvas?.redo();
		} else if (!mod && k === 'b') {
			eraser = false;
			tool = 'brush';
		} else if (!mod && k === 'e') {
			eraser = true;
			tool = 'brush';
		} else if (!mod && TOOL_KEYS[k]) tool = tool === TOOL_KEYS[k] ? 'brush' : TOOL_KEYS[k];
		else if (!mod && e.key === '[') size = Math.max(0.25, +(size / 1.25).toFixed(2));
		else if (!mod && e.key === ']') size = Math.min(6, +(size * 1.25).toFixed(2));
		else if (tool === 'transform' && e.key === 'Enter') canvas?.transformCommit();
		else if (tool === 'transform' && e.key === 'Escape') canvas?.transformCancel();
		else if (tool === 'pen' && e.key === 'Enter') {
			if (canvas?.penStroke(true)) canvas.penClear();
		} else if (tool === 'pen' && e.key === 'Escape') canvas?.penClear();
	}
</script>

<svelte:window onkeydown={onkey} />

<div class="app">
	<Studio tabs={TABS} bind:tab bind:dockOpen>
		{#snippet title()}
			<div class="brand">
				<Logo /><span data-testid="project-name">{projectName(path)}</span>
				{#if unsaved}<span class="dot" title="Not saved yet" aria-label="not saved">●</span>{/if}
			</div>
		{/snippet}
		{#snippet actions()}
			{#if fileNote}<span class="note" role="alert">{fileNote}</span>{/if}
			<button
				type="button"
				class="act"
				disabled={!!busy}
				onclick={open}
				title="Open a project (Ctrl+O)"
			>
				{busy === 'opening' ? 'Opening…' : 'Open'}
			</button>
			<button
				type="button"
				class="act"
				disabled={!!busy}
				onclick={() => save(false)}
				title="Save (Ctrl+S); Save as: Ctrl+Shift+S"
			>
				{busy === 'saving' ? 'Saving…' : 'Save'}
			</button>
			<VersionsMenu {path} disabled={!!busy} onopen={openOldVersion} />
			<div class="export">
				<button
					type="button"
					class="act"
					aria-expanded={exportOpen}
					disabled={!!busy || !status || status.blank}
					onclick={() => (exportOpen = !exportOpen)}
				>
					{busy === 'exporting' ? 'Exporting…' : 'Export'}
				</button>
				{#if exportOpen}
					<div class="menu" role="group" aria-label="Export as">
						<button
							type="button"
							onclick={() => {
								exportOpen = false;
								void exportAs('png');
							}}
						>
							<b>PNG picture</b><span>One flat image</span>
						</button>
						<button
							type="button"
							onclick={() => {
								exportOpen = false;
								void exportAs('psd');
							}}
						>
							<b>Photoshop file (.psd)</b><span>Keeps layers, groups and masks</span>
						</button>
					</div>
				{/if}
			</div>
			<button
				type="button"
				class="act"
				disabled={!status?.canUndo}
				onclick={() => canvas?.undo()}
				aria-label="Undo (Ctrl+Z)"
			>
				Undo
			</button>
			<button
				type="button"
				class="act"
				disabled={!status?.canRedo}
				onclick={() => canvas?.redo()}
				aria-label="Redo (Ctrl+Shift+Z)"
			>
				Redo
			</button>
			<a class="act" href={resolve('/pen-test')}>Pen test</a>
		{/snippet}
		{#snippet rail()}
			<ToolRail bind:tool bind:eraser bind:size bind:opacity />
		{/snippet}
		{#snippet options()}
			<span class="brush-name">{manifest ? brushLabel(manifest, brushId) : ''}</span>
			<Swatches bind:color onmore={() => ((tab = 'colour'), (dockOpen = true))} />
			<label class="slider">
				<span>Smoothing <b>{Math.round(smoothing * 100)}%</b></span>
				<input type="range" min="0" max="1" step="0.05" bind:value={smoothing} />
			</label>
		{/snippet}
		{#snippet stage()}
			{#key canvasKey}
				<PaintCanvas
					bind:this={canvas}
					fill
					width={canvasSize.width}
					height={canvasSize.height}
					{brushId}
					{eraser}
					{size}
					{opacity}
					{color}
					tool={tool as never}
					{smoothing}
					brushTexture={texture}
					allowImport
					onpick={(c) => (color = c)}
					onsize={(v) => (size = v)}
					onchange={(s) => (status = s)}
				/>
			{/key}
		{/snippet}
		{#snippet panel(t: string)}
			{#if t === 'layers' && status}
				<LayersPanel {status} controls={canvas} aspect={canvasSize.width / canvasSize.height} />
			{:else if t === 'brushes'}
				{#if manifest}
					<BrushPicker {manifest} bind:selected={brushId} />
					<BrushTexturePanel {brushId} onpick={(id) => (brushId = id)} />
				{:else}
					<p>Loading brushes…</p>
				{/if}
			{:else if t === 'history' && status}
				<HistoryPanel history={status.history} ongo={(n) => canvas?.historyGo(n)} />
			{:else if t === 'colour'}
				<ColorPanel bind:color onclose={() => (tab = 'layers')} />
			{/if}
		{/snippet}
		{#snippet after()}
			{#if recoverable.length && !recoverLater}
				<RecoverDialog
					items={recoverable}
					onrecover={recover}
					ondiscard={discard}
					onlater={() => (recoverLater = true)}
				/>
			{/if}
		{/snippet}
	</Studio>
</div>

<style>
	.app {
		height: 100vh;
		padding: 8px;
		box-sizing: border-box;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 10px;
		font: 700 18px var(--pp-font-display);
	}
	.act {
		min-height: 40px;
		padding: 0 14px;
		display: inline-grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
	}
	.act:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.export {
		position: relative;
	}
	.menu {
		position: absolute;
		z-index: 30;
		top: calc(100% + 8px);
		right: 0;
		display: grid;
		gap: 4px;
		width: 260px;
		padding: 8px;
		border-radius: 18px;
		background: var(--pp-surface);
		box-shadow: 0 12px 32px rgba(0, 0, 0, 0.18);
	}
	.menu button {
		display: grid;
		gap: 2px;
		padding: 10px 12px;
		border: 0;
		border-radius: 12px;
		background: none;
		color: var(--pp-ink);
		text-align: left;
		font: 14px var(--pp-font-body);
		cursor: pointer;
	}
	.menu button:hover {
		background: var(--pp-ground);
	}
	.menu span {
		color: var(--pp-muted);
		font-size: 13px;
	}
	.dot {
		color: var(--pp-accent);
		font-size: 12px;
	}
	.note {
		max-width: 320px;
		color: var(--pp-accent-ink);
		font-size: 13px;
	}
	.brush-name {
		font-weight: 700;
		white-space: nowrap;
	}
	.slider {
		display: grid;
		gap: 2px;
		width: 140px;
		font-size: 13px;
		color: var(--pp-on-ink-muted);
	}
	.slider b {
		color: var(--pp-on-ink);
	}
</style>
