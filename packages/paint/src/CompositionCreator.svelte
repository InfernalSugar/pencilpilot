<script lang="ts">
	/**
	 * Make a composition guide of your own: a window over the canvas with the guide's
	 * elements and look in groups, a preview over a sample picture that redraws as they
	 * change, and a little canvas to sketch on under it. Nothing changes in My
	 * compositions until Save.
	 */
	import { onMount, untrack } from 'svelte';
	import CompositionPreview from './CompositionPreview.svelte';
	import CompositionTestPad from './CompositionTestPad.svelte';
	import {
		compositionFile,
		compositionFileName,
		DEFAULT_COLOR,
		DEFAULT_OPACITY,
		defShapes,
		arcsPath,
		elementLabel,
		MAX_ELEMENTS,
		newElement,
		PHI_CUT,
		PRESET_NAMES,
		presetElements,
		type BuiltIn,
		type CompElement,
		type CompositionDef,
		type ElementKind,
		type Pt
	} from './composition-elements';
	import { deleteCustomComposition, saveCustomComposition } from './custom-compositions';

	let {
		start,
		canvasSize,
		askToDelete = true,
		ondone,
		onimport,
		onexport
	}: {
		/** one of My compositions to edit (or a project's own), or null for a new one */
		start: CompositionDef | null;
		/** this canvas's size (the preview can take its shape) */
		canvasSize: { width: number; height: number };
		/** ask "Delete this composition?" first */
		askToDelete?: boolean;
		/** closed: the guide saved (to use now), or null to keep the one in use */
		ondone: (saved: CompositionDef | null, deleted?: string) => void;
		/** files chosen to import: how it went */
		onimport?: (files: File[]) => Promise<string>;
		/** save the file of a guide (its name, its text): how it went */
		onexport?: (name: string, text: string) => Promise<string>;
	} = $props();

	// (read once: the window edits the guide it was opened on)
	const editing = untrack(() => (start?.id ? start : null));
	const fresh = untrack(() => !start);
	const begin = untrack(() => ({
		elements: structuredClone($state.snapshot(start?.elements ?? [])) as CompElement[],
		color: start?.color ?? DEFAULT_COLOR,
		opacity: start?.opacity ?? DEFAULT_OPACITY
	}));

	let elements = $state<CompElement[]>(structuredClone(begin.elements));
	let color = $state(begin.color);
	let opacity = $state(begin.opacity);
	let name = $state(untrack(() => start?.name ?? 'My composition'));
	let picked = $state(0);
	let confirmDelete = $state(false);
	let note = $state('');

	const title = fresh ? 'New composition' : `Edit “${untrack(() => start!.name)}”`;

	onMount(() => {
		if (dialog && !dialog.open) {
			dialog.showModal();
			dialog.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
		}
	});

	/** the guide as set now */
	const current = (): Omit<CompositionDef, 'id'> => ({
		name: name.trim() || 'My composition',
		color,
		opacity,
		elements: $state.snapshot(elements) as CompElement[]
	});

	function save(copy: boolean) {
		const now = current();
		if (copy && editing && now.name === editing.name) now.name = `${now.name} copy`;
		ondone(saveCustomComposition({ ...now, id: copy ? undefined : editing?.id }));
	}

	function startOver() {
		elements = structuredClone(begin.elements);
		color = begin.color;
		opacity = begin.opacity;
		picked = 0;
	}

	function remove() {
		if (!editing) return;
		deleteCustomComposition(editing.id);
		ondone(null, editing.id);
	}

	async function exportIt() {
		const now = current();
		note = onexport ? await onexport(compositionFileName(now.name), compositionFile([now])) : '';
	}

	// ---- elements ----

	const KINDS: { kind: ElementKind; label: string }[] = [
		{ kind: 'grid', label: 'Grid' },
		{ kind: 'ratio', label: 'Ratio line' },
		{ kind: 'line', label: 'Line' },
		{ kind: 'diagonals', label: 'Diagonals' },
		{ kind: 'spiral', label: 'Spiral' },
		{ kind: 'frame', label: 'Frame' },
		{ kind: 'ellipse', label: 'Circle or ellipse' }
	];
	function add(kind: ElementKind) {
		if (elements.length >= MAX_ELEMENTS) return;
		elements.push(newElement(kind));
		picked = elements.length - 1;
	}
	function drop(i: number) {
		elements.splice(i, 1);
		picked = Math.min(picked, elements.length - 1);
	}
	const el = $derived(elements[picked] as CompElement | undefined);

	function startFrom(p: BuiltIn | null) {
		elements = p ? presetElements(p, frame.w / frame.h) : [];
		picked = 0;
		group = 'elements';
	}

	// ---- the preview's frame ----

	const FRAMES = [
		{ id: 'canvas', label: 'This canvas' },
		{ id: '16:9', label: '16:9', r: 16 / 9 },
		{ id: '4:3', label: '4:3', r: 4 / 3 },
		{ id: '1:1', label: '1:1', r: 1 },
		{ id: '9:16', label: '9:16', r: 9 / 16 },
		{ id: 'a4', label: 'A4', r: 210 / 297 }
	];
	let frameId = $state('canvas');
	/** the preview's size in px: the frame's shape inside 440 × 250 */
	const frame = $derived.by(() => {
		const f = FRAMES.find((x) => x.id === frameId)!;
		const r = f.r ?? canvasSize.width / Math.max(1, canvasSize.height);
		const [bw, bh] = [440, 250];
		return r >= bw / bh ? { w: bw, h: Math.round(bw / r) } : { w: Math.round(bh * r), h: bh };
	});

	function point(i: number, end: 'a' | 'b', p: Pt) {
		const e = elements[i];
		if (e?.kind === 'line') e[end] = p;
	}

	// ---- groups ----

	const GROUPS = [
		{ id: 'elements', label: 'Elements' },
		{ id: 'look', label: 'Colour and lines' },
		{ id: 'start', label: 'Start from' }
	];
	let group = $state('elements');

	const pct = (v: number) => `${Math.round(v * 100)}%`;

	let dialog = $state<HTMLDialogElement>();
	let downOutside = false;
	let files = $state<HTMLInputElement>();
</script>

{#snippet slider(
	label: string,
	value: number,
	min: number,
	max: number,
	step: number,
	show: string,
	set: (v: number) => void
)}
	<label class="row">
		<span class="lbl">{label} <b>{show}</b></span>
		<input
			type="range"
			{min}
			{max}
			{step}
			{value}
			aria-label={label}
			oninput={(e) => set(+e.currentTarget.value)}
		/>
	</label>
{/snippet}

{#snippet choice(
	label: string,
	options: [string, string][],
	value: string,
	set: (v: string) => void
)}
	<div class="choice">
		<span class="lbl">{label}</span>
		<div class="seg" role="radiogroup" aria-label={label}>
			{#each options as [id, text] (id)}
				<button type="button" role="radio" aria-checked={value === id} onclick={() => set(id)}>
					{text}
				</button>
			{/each}
		</div>
	</div>
{/snippet}

{#snippet check(label: string, value: boolean, set: (v: boolean) => void)}
	<label class="check">
		<input type="checkbox" checked={value} onchange={(e) => set(e.currentTarget.checked)} />
		<span>{label}</span>
	</label>
{/snippet}

<dialog
	bind:this={dialog}
	class="creator"
	aria-labelledby="comp-creator-h"
	oncancel={(e) => {
		e.preventDefault();
		ondone(null);
	}}
	onpointerdown={(e) => (downOutside = e.target === dialog)}
	onclick={(e) => {
		// a click on the dimmed canvas around the window closes it (not a drag out of it)
		if (e.target === dialog && downOutside) ondone(null);
	}}
	onkeydown={(e) => {
		// the canvas's shortcuts wait while this is open (Esc still closes it)
		e.stopPropagation();
	}}
>
	<div class="inner">
		<header>
			<h1 id="comp-creator-h">{title}</h1>
			<label class="name">
				<span>Name</span>
				<input type="text" maxlength="40" bind:value={name} aria-label="Composition name" />
			</label>
			{#if onimport}
				<button type="button" class="ghost" onclick={() => files?.click()}>
					Import compositions…
				</button>
				<input
					bind:this={files}
					class="hidden"
					type="file"
					multiple
					accept=".json,application/json"
					aria-label="Composition files to import"
					onchange={(e) => {
						const list = [...(e.currentTarget.files ?? [])];
						e.currentTarget.value = '';
						if (!list.length) return;
						note = 'Reading…';
						void onimport(list).then((n) => (note = n));
					}}
				/>
			{/if}
			<button
				type="button"
				class="ghost close"
				aria-label="Close without saving"
				onclick={() => ondone(null)}
			>
				×
			</button>
		</header>

		{#if note}<p class="note" role="status">{note}</p>{/if}
		<div class="split">
			<div
				class="groups"
				role="tablist"
				aria-label="Composition settings"
				aria-orientation="vertical"
			>
				{#each GROUPS as g (g.id)}
					<button
						type="button"
						role="tab"
						id="comp-tab-{g.id}"
						aria-selected={group === g.id}
						aria-controls="comp-settings"
						onclick={() => (group = g.id)}
					>
						{g.label}
					</button>
				{/each}
			</div>

			<div class="settings" id="comp-settings" role="tabpanel" aria-labelledby="comp-tab-{group}">
				{#if group === 'elements'}
					<h2>Elements</h2>
					{#if elements.length}
						<ul class="elements" aria-label="Elements">
							{#each elements as e, i (i)}
								<li class:picked={picked === i}>
									<input
										type="checkbox"
										checked={e.on}
										aria-label="Show {elementLabel(e)}"
										onchange={(ev) => (e.on = ev.currentTarget.checked)}
									/>
									<button
										type="button"
										class="pick"
										aria-pressed={picked === i}
										onclick={() => (picked = i)}
									>
										{elementLabel(e)}{e.style === 'dashed' ? ' · dashed' : ''}
									</button>
									<button
										type="button"
										class="remove"
										aria-label="Remove {elementLabel(e)}"
										title="Remove"
										onclick={() => drop(i)}
									>
										×
									</button>
								</li>
							{/each}
						</ul>
					{:else}
						<p class="hint">No elements yet: add one below, or start from a built-in guide.</p>
					{/if}
					<div class="add" role="group" aria-label="Add an element">
						<span class="lbl">Add</span>
						{#each KINDS as k (k.kind)}
							<button
								type="button"
								disabled={elements.length >= MAX_ELEMENTS}
								onclick={() => add(k.kind)}
							>
								{k.label}
							</button>
						{/each}
					</div>
					{#if el}
						<fieldset class="edit" aria-label="Element settings">
							<h3>{elementLabel(el)}</h3>
							{#if el.kind === 'grid'}
								{@const g = el}
								{@render slider('Columns', g.cols, 1, 24, 1, `${g.cols}`, (v) => (g.cols = v))}
								{@render slider('Rows', g.rows, 1, 24, 1, `${g.rows}`, (v) => (g.rows = v))}
							{:else if el.kind === 'ratio'}
								{@const r = el}
								{@render choice(
									'Ratio',
									[
										['golden', 'Golden (0.618)'],
										['third', '1/3'],
										['root2', '√2 (0.707)'],
										['custom', 'Custom']
									],
									r.ratio,
									(v) => (r.ratio = v as typeof r.ratio)
								)}
								{#if r.ratio === 'custom'}
									{@render slider('At', r.at, 0.01, 0.99, 0.01, pct(r.at), (v) => (r.at = v))}
								{/if}
								{@render choice(
									'Direction',
									[
										['vertical', 'Up and down'],
										['horizontal', 'Across']
									],
									r.dir,
									(v) => (r.dir = v as typeof r.dir)
								)}
								{@render choice(
									'Measured from',
									r.dir === 'vertical'
										? [
												['start', 'Left'],
												['end', 'Right']
											]
										: [
												['start', 'Top'],
												['end', 'Bottom']
											],
									r.from,
									(v) => (r.from = v as typeof r.from)
								)}
							{:else if el.kind === 'line'}
								{@const l = el}
								<p class="hint">Drag its ends in the preview, or set them here.</p>
								{@render slider(
									'Start across',
									l.a[0],
									0,
									1,
									0.005,
									pct(l.a[0]),
									(v) => (l.a = [v, l.a[1]])
								)}
								{@render slider(
									'Start down',
									l.a[1],
									0,
									1,
									0.005,
									pct(l.a[1]),
									(v) => (l.a = [l.a[0], v])
								)}
								{@render slider(
									'End across',
									l.b[0],
									0,
									1,
									0.005,
									pct(l.b[0]),
									(v) => (l.b = [v, l.b[1]])
								)}
								{@render slider(
									'End down',
									l.b[1],
									0,
									1,
									0.005,
									pct(l.b[1]),
									(v) => (l.b = [l.b[0], v])
								)}
							{:else if el.kind === 'diagonals'}
								{@const d = el}
								{@render check('Corner to corner', d.corners, (v) => (d.corners = v))}
								{@render check('45° from the corners', d.angled, (v) => (d.angled = v))}
							{:else if el.kind === 'spiral'}
								{@const s = el}
								{@render slider(
									'Ratio',
									s.ratio,
									0.4,
									0.75,
									0.001,
									s.ratio.toFixed(3),
									(v) => (s.ratio = v)
								)}
								<button
									type="button"
									class="ghost small"
									disabled={Math.abs(s.ratio - PHI_CUT) < 1e-6}
									onclick={() => (s.ratio = PHI_CUT)}
								>
									Golden
								</button>
								{@render check('Flip left to right', s.flipX, (v) => (s.flipX = v))}
								{@render check('Flip top to bottom', s.flipY, (v) => (s.flipY = v))}
							{:else if el.kind === 'frame'}
								{@const f = el}
								{@render slider('Inset', f.inset, 0, 45, 0.5, `${f.inset}%`, (v) => (f.inset = v))}
								<p class="hint">Safe areas: 5% for action, 10% for titles.</p>
							{:else if el.kind === 'ellipse'}
								{@const o = el}
								{@render check('Circle', o.circle, (v) => (o.circle = v))}
								{@render slider(
									'Centre across',
									o.c[0],
									0,
									1,
									0.005,
									pct(o.c[0]),
									(v) => (o.c = [v, o.c[1]])
								)}
								{@render slider(
									'Centre down',
									o.c[1],
									0,
									1,
									0.005,
									pct(o.c[1]),
									(v) => (o.c = [o.c[0], v])
								)}
								{#if o.circle}
									{@render slider('Size', o.rx, 0.01, 1, 0.005, pct(o.rx), (v) => (o.rx = v))}
								{:else}
									{@render slider('Width', o.rx, 0.01, 1, 0.005, pct(o.rx), (v) => (o.rx = v))}
									{@render slider('Height', o.ry, 0.01, 1, 0.005, pct(o.ry), (v) => (o.ry = v))}
								{/if}
							{/if}
							{@render choice(
								'Line',
								[
									['solid', 'Solid'],
									['dashed', 'Dashed']
								],
								el.style,
								(v) => el && (el.style = v as 'solid' | 'dashed')
							)}
						</fieldset>
					{/if}
				{:else if group === 'look'}
					<h2>Colour and lines</h2>
					<label class="colour-row">
						<span class="lbl">Colour</span>
						<input type="color" bind:value={color} aria-label="Guide colour" />
					</label>
					{@render slider('Opacity', opacity, 0.05, 1, 0.01, pct(opacity), (v) => (opacity = v))}
					<p class="hint">Each element is solid or dashed: pick it in Elements.</p>
				{:else}
					<h2>Start from</h2>
					<p class="hint">Replaces the elements with a built-in guide’s, to change as you like.</p>
					<div class="starts">
						<button type="button" class="start" onclick={() => startFrom(null)}>
							<span class="blank" aria-hidden="true"></span>
							Blank
						</button>
						{#each Object.keys(PRESET_NAMES) as BuiltIn[] as p (p)}
							{@const s = defShapes({ elements: presetElements(p, 1.5) }, 48, 32).solid}
							<button type="button" class="start" onclick={() => startFrom(p)}>
								<svg viewBox="-1 -1 50 34" width="50" height="34" aria-hidden="true">
									<rect x="0" y="0" width="48" height="32" />
									{#each s.lines as [x1, y1, x2, y2], i (i)}
										<line {x1} {y1} {x2} {y2} />
									{/each}
									{#if s.arcs.length}<path d={arcsPath(s.arcs)} />{/if}
								</svg>
								{PRESET_NAMES[p]}
							</button>
						{/each}
					</div>
				{/if}
			</div>

			<div class="try">
				<div class="try-head">
					<h2>Preview</h2>
					<div class="seg frames" role="radiogroup" aria-label="Frame">
						{#each FRAMES as f (f.id)}
							<button
								type="button"
								role="radio"
								aria-checked={frameId === f.id}
								onclick={() => (frameId = f.id)}
							>
								{f.label}
							</button>
						{/each}
					</div>
				</div>
				<CompositionPreview
					{elements}
					{color}
					{opacity}
					width={frame.w}
					height={frame.h}
					selected={picked}
					onpoint={point}
				/>
				<h2 class="gap">Try it</h2>
				{#key `${frame.w}x${frame.h}`}
					<CompositionTestPad {elements} {color} {opacity} width={frame.w} height={frame.h} />
				{/key}
				<p class="hint">Sketch under the guide to see how it works in use.</p>
			</div>
		</div>

		<footer>
			{#if editing}
				{#if confirmDelete}
					<span class="warn">Delete this composition?</span>
					<button type="button" class="ghost danger" onclick={remove}>Delete</button>
					<button type="button" class="ghost" onclick={() => (confirmDelete = false)}>Keep</button>
				{:else}
					<button
						type="button"
						class="ghost"
						onclick={() => (askToDelete ? (confirmDelete = true) : remove())}
					>
						Delete composition
					</button>
				{/if}
			{/if}
			<button type="button" class="ghost" onclick={startOver}>Start over</button>
			{#if onexport}
				<button type="button" class="ghost" onclick={exportIt}>Export…</button>
			{/if}
			<span class="space"></span>
			<button type="button" class="ghost" onclick={() => ondone(null)}>Cancel</button>
			{#if editing}
				<button type="button" class="ghost" onclick={() => save(true)}>Save as a copy</button>
			{/if}
			<button type="button" class="primary" onclick={() => save(false)}>
				{editing ? 'Save' : 'Save to My compositions'}
			</button>
		</footer>
	</div>
</dialog>

<style>
	.creator {
		width: min(1120px, calc(100vw - 32px));
		height: min(800px, calc(100vh - 32px));
		max-width: none;
		max-height: none;
		padding: 0;
		border: 0;
		border-radius: 24px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.35);
	}
	.creator::backdrop {
		background: rgba(22, 22, 26, 0.25);
	}
	/* (the padding is inside: a click on the window itself is a click outside it) */
	.inner {
		display: flex;
		flex-direction: column;
		gap: 12px;
		height: 100%;
		box-sizing: border-box;
		padding: 18px 22px;
		font: 14px var(--pp-font-body);
	}
	header {
		flex: none;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 12px;
	}
	h1 {
		flex: 1 1 auto;
		min-width: 0;
		margin: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 800 24px var(--pp-font-display);
	}
	h2 {
		margin: 0 0 8px;
		font: 700 17px var(--pp-font-display);
	}
	h3 {
		margin: 0 0 8px;
		font: 700 14px var(--pp-font-body);
	}
	.gap {
		margin-top: 8px;
	}
	.name {
		display: flex;
		align-items: center;
		gap: 8px;
		font-weight: 600;
	}
	.name input {
		width: 200px;
		min-height: 38px;
		padding: 0 10px;
		box-sizing: border-box;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 500 14px var(--pp-font-body);
	}
	.hidden {
		display: none;
	}
	.split {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: 170px minmax(0, 1fr) minmax(0, 480px);
		gap: 14px;
	}
	.groups {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.groups button {
		padding: 10px 12px;
		border: 0;
		border-radius: 12px;
		background: none;
		color: var(--pp-ink-2);
		font: 600 14px var(--pp-font-body);
		text-align: left;
		cursor: pointer;
	}
	.groups button:hover {
		background: var(--pp-surface-sunk);
	}
	.groups button[aria-selected='true'] {
		background: var(--pp-surface);
		color: var(--pp-ink);
		box-shadow: inset 3px 0 0 var(--pp-accent);
	}
	.settings,
	.try {
		min-height: 0;
		overflow-y: auto;
		padding: 16px 18px;
		border-radius: 18px;
		background: var(--pp-surface);
		scrollbar-width: thin;
	}
	.try {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.try-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 6px;
	}
	.try-head h2 {
		margin: 0;
	}
	.elements {
		list-style: none;
		margin: 0 0 12px;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.elements li {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 2px 4px 2px 8px;
		border-radius: 10px;
	}
	.elements li.picked {
		background: var(--pp-ground);
		box-shadow: inset 3px 0 0 var(--pp-accent);
	}
	.elements input {
		accent-color: var(--pp-accent-strong);
	}
	.pick {
		flex: 1;
		min-width: 0;
		min-height: 32px;
		padding: 0 6px;
		border: 0;
		background: none;
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		text-align: left;
		cursor: pointer;
	}
	.remove {
		width: 30px;
		height: 30px;
		border: 0;
		border-radius: 999px;
		background: none;
		color: var(--pp-muted);
		font-size: 18px;
		cursor: pointer;
	}
	.remove:hover {
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
	}
	.add {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		margin: 0 0 14px;
	}
	.add button,
	.seg button,
	.small {
		min-height: 32px;
		padding: 0 10px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.seg {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.seg button[aria-checked='true'] {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.frames button {
		min-height: 28px;
		padding: 0 8px;
		font-size: 12px;
	}
	.edit {
		margin: 0;
		padding: 12px 0 0;
		border: 0;
		border-top: 1px solid var(--pp-surface-sunk);
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin: 0 0 12px;
	}
	.row input {
		accent-color: var(--pp-accent-strong);
	}
	.lbl {
		display: flex;
		justify-content: space-between;
		gap: 8px;
		font-size: 13px;
		font-weight: 700;
	}
	.lbl b {
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	.choice {
		display: flex;
		flex-direction: column;
		gap: 4px;
		margin: 0 0 12px;
	}
	.check {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 4px 0 10px;
		font-size: 13px;
		font-weight: 600;
	}
	.check input {
		accent-color: var(--pp-accent-strong);
	}
	.small {
		margin: -4px 0 10px;
	}
	.colour-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin: 0 0 12px;
	}
	.colour-row input {
		width: 48px;
		height: 32px;
		padding: 2px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 8px;
		background: var(--pp-ground);
	}
	.starts {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
		gap: 8px;
		margin-top: 10px;
	}
	.start {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding: 8px 6px;
		border: 2px solid transparent;
		border-radius: 14px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font: 500 12px/1.2 var(--pp-font-body);
		cursor: pointer;
	}
	.start:hover {
		border-color: var(--pp-accent);
	}
	.start svg {
		fill: none;
		stroke: currentColor;
		stroke-width: 1;
	}
	.start svg rect {
		stroke-opacity: 0.5;
	}
	.blank {
		width: 48px;
		height: 32px;
		margin: 1px;
		border: 1px solid currentColor;
		opacity: 0.5;
	}
	.hint {
		margin: 0 0 10px;
		color: var(--pp-muted);
		font-size: 12px;
		font-weight: 400;
	}
	footer {
		flex: none;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px;
	}
	.space {
		flex: 1;
	}
	.primary,
	.ghost {
		min-height: 40px;
		padding: 0 14px;
		border-radius: 12px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.primary {
		border: 0;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.ghost {
		border: 0;
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	.ghost.small {
		min-height: 32px;
		background: var(--pp-ground);
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.close {
		width: 40px;
		padding: 0;
		font-size: 20px;
	}
	.danger {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.note {
		margin: -4px 0 0;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.warn {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-ink);
	}

	/* narrower windows: the groups go across the top, the try-out area below */
	@media (max-width: 1000px) {
		.split {
			grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
			grid-template-rows: auto minmax(0, 1fr);
		}
		.groups {
			grid-column: 1 / -1;
			flex-direction: row;
			flex-wrap: wrap;
		}
		.groups button[aria-selected='true'] {
			box-shadow: inset 0 -3px 0 var(--pp-accent);
		}
	}
	@media (max-width: 700px) {
		.split {
			grid-template-columns: minmax(0, 1fr);
			grid-template-rows: auto auto auto;
			overflow-y: auto;
		}
		.settings,
		.try {
			overflow: visible;
		}
	}
</style>
