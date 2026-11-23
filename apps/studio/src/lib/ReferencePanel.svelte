<script lang="ts">
	/**
	 * The reference pictures: the user's own pictures to draw from, imported into the
	 * project. Shown in one of three places (as the website does): the corner (the top of
	 * the side panel), the side (a pane beside the canvas, resized with its divider) or a
	 * window that pops out over everything and can be moved and resized.
	 * The viewer is the website's (zoom, pan, flip, colour picker, grid).
	 */
	import { untrack } from 'svelte';
	import RefViewer from '@pencilpilot/paint/RefViewer.svelte';
	import { REF_PLACES, type RefLayout, type RefPlace } from './settings.svelte';
	import { gridOnPicture, skippedNote, type RefShelf } from './references.svelte';

	let {
		shelf,
		where,
		layout = $bindable(),
		gridLines = null,
		canvasSize,
		onpick,
		onchange
	}: {
		shelf: RefShelf;
		/** the place this one shows in */
		where: RefPlace;
		/** settings.reference: where it shows, how big */
		layout: RefLayout;
		/** the canvas grid's lines when the grid is on (put on the picture too) */
		gridLines?: [number, number, number, number][] | null;
		canvasSize: { width: number; height: number };
		/** a colour was picked from the picture */
		onpick?: (hex: string) => void;
		/** pictures were added or taken away (the project changed) */
		onchange?: () => void;
	} = $props();

	const PLACE_LABEL: Record<RefPlace, [string, string]> = {
		corner: ['Corner', 'At the top of the side panel'],
		side: ['Side', 'In its own pane beside the canvas (drag the divider to resize)'],
		float: ['Pop out', 'In a window you can move and resize']
	};

	let input = $state<HTMLInputElement>();
	let note = $state('');
	let busy = $state(false);
	/** how the picture's values show: as it is, grey, or 2 / 3 flat tones; blurred (squint) */
	let values = $state<'colour' | 'grey' | 2 | 3>('colour');
	let squint = $state(false);

	const pic = $derived(shelf.pic);
	const grid = $derived(pic && gridLines ? gridOnPicture(gridLines, canvasSize, pic) : null);

	async function importFiles(files: File[]) {
		if (!files.length) return;
		busy = true;
		note = '';
		try {
			const read = await Promise.all(
				files.map(async (f) => ({ name: f.name, bytes: new Uint8Array(await f.arrayBuffer()) }))
			);
			const r = await shelf.add(read);
			note = skippedNote(r);
			if (r.added) onchange?.();
		} catch (e) {
			note = `Couldn’t import: ${e}`;
		} finally {
			busy = false;
		}
	}

	function remove() {
		shelf.remove(shelf.current);
		note = '';
		onchange?.();
	}

	// ---- the pop-out window: dragged by its bar, resized by its corner ----
	let drag: { x: number; y: number; bx: number; by: number } | null = null;
	function onScreen() {
		const f = layout.float;
		const x = Math.max(0, Math.min(window.innerWidth - 120, f.x));
		const y = Math.max(0, Math.min(window.innerHeight - 60, f.y));
		if (x !== f.x || y !== f.y) layout.float = { ...f, x, y };
	}
	$effect(() => {
		if (where !== 'float') return;
		untrack(onScreen);
		window.addEventListener('resize', onScreen);
		return () => window.removeEventListener('resize', onScreen);
	});
</script>

<section
	class="refs {where}"
	aria-label="Reference pictures"
	data-testid="reference-{where}"
	style={where === 'float'
		? `left: ${layout.float.x}px; top: ${layout.float.y}px; width: ${layout.float.w}px; height: ${layout.float.h}px`
		: undefined}
	onpointerup={(e) => {
		if (where !== 'float' || e.currentTarget !== e.target) return;
		// the window's own resize corner: keep the size it was given
		const el = e.currentTarget as HTMLElement;
		layout.float = { ...layout.float, w: el.offsetWidth, h: el.offsetHeight };
	}}
>
	<div
		class="bar"
		role="presentation"
		onpointerdown={(e) => {
			if (where !== 'float' || (e.target as HTMLElement).closest('button, select')) return;
			e.currentTarget.setPointerCapture(e.pointerId);
			drag = { x: e.clientX, y: e.clientY, bx: layout.float.x, by: layout.float.y };
		}}
		onpointermove={(e) => {
			if (!drag) return;
			layout.float = {
				...layout.float,
				x: drag.bx + e.clientX - drag.x,
				y: drag.by + e.clientY - drag.y
			};
		}}
		onpointerup={() => {
			if (!drag) return;
			drag = null;
			onScreen();
		}}
	>
		<span class="title" title={where === 'float' ? 'Drag to move' : undefined}>
			Reference
			{#if shelf.pics.length > 1}<small>{shelf.current + 1} of {shelf.pics.length}</small>{/if}
		</span>
		{#if shelf.pics.length > 1}
			<button
				type="button"
				class="icon"
				aria-label="Previous picture"
				onclick={() => shelf.step(-1)}
			>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 6-6 6 6 6" /></svg>
			</button>
			<button type="button" class="icon" aria-label="Next picture" onclick={() => shelf.step(1)}>
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
			</button>
		{/if}
		<button
			type="button"
			class="icon"
			aria-label="Import pictures"
			title="Import your own pictures to draw from"
			disabled={busy}
			onclick={() => input?.click()}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
		</button>
		<button
			type="button"
			class="icon"
			aria-label="Hide the reference"
			onclick={() => (layout.shown = false)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
		</button>
	</div>
	<div class="places" role="radiogroup" aria-label="Where the reference shows">
		{#each REF_PLACES as p (p)}
			<button
				type="button"
				role="radio"
				aria-checked={layout.place === p}
				title={PLACE_LABEL[p][1]}
				onclick={() => (layout.place = p)}
			>
				{PLACE_LABEL[p][0]}
			</button>
		{/each}
	</div>
	{#if pic}
		{#key pic.id}
			<RefViewer
				src={pic.url}
				alt="Reference: {pic.name}"
				fill={where !== 'corner'}
				gray={values === 'grey'}
				notan={values === 2 || values === 3 ? values : 0}
				{squint}
				{grid}
				{onpick}
			/>
		{/key}
		<div class="row">
			<label class="values">
				<span>Values</span>
				<select bind:value={values} aria-label="How the picture’s values show">
					<option value="colour">Colour</option>
					<option value="grey">Grey</option>
					<option value={3}>3 tones</option>
					<option value={2}>2 tones</option>
				</select>
			</label>
			<button
				type="button"
				class="chip"
				aria-pressed={squint}
				title="Blur the picture, to see its big shapes"
				onclick={() => (squint = !squint)}
			>
				Squint
			</button>
			<button
				type="button"
				class="chip quiet"
				title="Take {pic.name} out of this project"
				onclick={remove}
			>
				Remove
			</button>
		</div>
		{#if shelf.pics.length > 1}
			<div class="strip" role="listbox" aria-label="Reference pictures">
				{#each shelf.pics as p, i (p.id)}
					<button
						type="button"
						role="option"
						aria-selected={i === shelf.current}
						title={p.name}
						onclick={() => (shelf.current = i)}
					>
						<img src={p.url} alt={p.name} draggable="false" />
					</button>
				{/each}
			</div>
		{/if}
	{:else}
		<div class="empty">
			<p>
				Import your own pictures to draw from. They’re kept in this project, and never part of the
				drawing or its exports.
			</p>
			<button type="button" class="chip strong" disabled={busy} onclick={() => input?.click()}>
				{busy ? 'Importing…' : 'Import pictures…'}
			</button>
		</div>
	{/if}
	{#if note}<p class="note" role="status">{note}</p>{/if}
	<input
		bind:this={input}
		type="file"
		accept="image/*"
		multiple
		hidden
		aria-label="Reference picture files"
		onchange={(e) => {
			const files = [...(e.currentTarget.files ?? [])];
			e.currentTarget.value = '';
			void importFiles(files);
		}}
	/>
</section>

<style>
	.refs {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-width: 0;
		min-height: 0;
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		--ref-max-h: 40vh;
	}
	.refs.side {
		height: 100%;
		padding: 8px;
		box-sizing: border-box;
		background: var(--pp-surface);
	}
	.refs.corner {
		padding: 12px;
		border-radius: 18px;
		background: var(--pp-surface);
	}
	.refs.float {
		position: fixed;
		z-index: 30;
		min-width: 200px;
		min-height: 200px;
		max-width: calc(100vw - 16px);
		max-height: calc(100vh - 16px);
		padding: 0 12px 12px;
		box-sizing: border-box;
		border-radius: 20px;
		background: var(--pp-surface);
		box-shadow: 0 10px 40px rgba(22, 22, 26, 0.25);
		resize: both;
		overflow: hidden;
	}
	.side :global(.ref),
	.float :global(.ref) {
		flex: 1;
	}
	.bar {
		flex: none;
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.float .bar {
		padding-top: 10px;
		cursor: move;
		touch-action: none;
	}
	.title {
		flex: 1;
		min-width: 0;
		font-size: 14px;
		font-weight: 700;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.title small {
		margin-left: 4px;
		font-weight: 600;
		color: var(--pp-ink-2);
	}
	button {
		font: inherit;
		color: inherit;
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.icon {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		padding: 0;
		border: 0;
		border-radius: 10px;
		background: transparent;
	}
	.icon:hover:not(:disabled) {
		background: var(--pp-surface-sunk);
	}
	.icon svg {
		width: 18px;
		height: 18px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.places {
		flex: none;
		display: flex;
		padding: 3px;
		gap: 2px;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
	}
	.places button {
		flex: 1;
		min-height: 28px;
		border: 0;
		border-radius: 9px;
		background: transparent;
	}
	.places button[aria-checked='true'] {
		background: var(--pp-surface);
		box-shadow: 0 1px 3px rgba(22, 22, 26, 0.18);
	}
	.row {
		flex: none;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.values {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-right: auto;
	}
	.values span {
		color: var(--pp-ink-2);
	}
	select {
		min-height: 30px;
		padding: 0 8px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: inherit;
	}
	.chip {
		min-height: 30px;
		padding: 0 12px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
	}
	.chip[aria-pressed='true'],
	.chip.strong {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.chip.quiet {
		background: transparent;
		color: var(--pp-ink-2);
	}
	.strip {
		flex: none;
		display: flex;
		gap: 6px;
		overflow-x: auto;
		padding-bottom: 2px;
	}
	.strip button {
		flex: none;
		width: 52px;
		height: 52px;
		padding: 0;
		border: 2px solid transparent;
		border-radius: 10px;
		overflow: hidden;
		background: var(--pp-surface-sunk);
	}
	.strip button[aria-selected='true'] {
		border-color: var(--pp-accent);
	}
	.strip img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.empty {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		justify-content: center;
		gap: 10px;
		padding: 16px;
		border-radius: 18px;
		background: var(--pp-surface-sunk);
	}
	.empty p {
		margin: 0;
		font-weight: 500;
		line-height: 1.45;
		color: var(--pp-ink-2);
	}
	.note {
		margin: 0;
		font-weight: 500;
		color: var(--pp-ink-2);
	}
</style>
