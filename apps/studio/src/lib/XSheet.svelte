<script lang="ts">
	/**
	 * The exposure sheet (X-sheet): the cut read down the page, a row per frame, with
	 * columns for the action, the dialogue and each layer (top layer first, as in the
	 * timeline). A drawing's number is written where it starts; a line runs down while
	 * it's held; × is an empty cel.
	 *
	 * Like a paper sheet, you write on it: pick a cell and type a drawing number (Enter
	 * or ↓ goes on to the next frame), X or Delete for an empty cel, and type notes into
	 * the action and dialogue columns. The arrow keys move around.
	 */
	import { untrack } from 'svelte';
	import type { LayerInfo } from '@pencilpilot/paint/PaintCanvas.svelte';
	import {
		cutLength,
		drawingNumbers,
		exposureAt,
		setKey,
		setNote,
		setStaticDrawing,
		staticDrawing,
		trackIn,
		type Timeline
	} from './timeline';

	let {
		timeline,
		scene = 0,
		cut = 0,
		frame = $bindable(0),
		fps = 24,
		layers,
		active,
		newDrawing,
		onlayer,
		onedit
	}: {
		timeline: Timeline;
		scene?: number;
		cut?: number;
		frame?: number;
		fps?: number;
		/** bottom → top, as the canvas lists them */
		layers: LayerInfo[];
		active: number;
		/** a new, empty drawing's id (from the canvas) */
		newDrawing: () => number;
		onlayer: (id: number) => void;
		/** the timeline changed (or the frame did): show it */
		onedit: (changed: boolean) => void;
	} = $props();

	type Col = 'action' | 'dialogue' | number;

	const theScene = $derived(timeline.scenes[scene] ?? timeline.scenes[0]);
	const theCut = $derived(theScene.cuts[cut] ?? theScene.cuts[0]);
	const statics = $derived(timeline.statics);
	const length = $derived(cutLength(theCut, statics));
	/** room to write ahead */
	const shown = $derived(Math.max(length + 24, 48));
	const cols = $derived<Col[]>([
		'action',
		'dialogue',
		...[...layers]
			.reverse()
			.filter((l) => !l.adjust)
			.map((l) => l.id)
	]);

	/** the cell picked (its column; the row is the current frame) */
	let col = $state<Col>('action');
	// the active layer's column, when another layer becomes active
	$effect(() => {
		const a = active;
		untrack(() => {
			if (typeof col === 'number' && col !== a) {
				col = a;
				typed = '';
			} else if (col === 'action' && !picked) col = a;
		});
	});
	/** a cell has been picked by hand (before that, the sheet starts on the active layer) */
	let picked = false;
	/** a drawing number being typed */
	let typed = $state('');
	/** a note being written (its text) */
	let writing = $state<string | null>(null);
	let note = $state('');
	let sheetEl = $state<HTMLElement>();

	/** What a layer's cell shows on a frame. */
	function cell(
		layer: number,
		f: number
	): { text: string; kind: 'key' | 'hold' | 'empty' | ''; mark?: string } {
		const t = theCut.tracks[layer] ?? [];
		if (statics.includes(layer)) {
			const d = staticDrawing(t);
			if (!d || f >= length) return { text: '', kind: '' };
			return f === 0 ? { text: 'S', kind: 'key' } : { text: '', kind: 'hold' };
		}
		const at = exposureAt(t, f);
		if (!at) return { text: '', kind: '' };
		const d = t[at.index].drawing;
		if (at.start !== f) return { text: '', kind: d ? 'hold' : '' };
		if (!d) return { text: '×', kind: 'empty' };
		return { text: String(nums(layer).get(d) ?? '?'), kind: 'key', mark: timeline.kinds?.[d] };
	}
	const MARK_NAME: Record<string, string> = {
		key: 'key',
		breakdown: 'breakdown',
		inbetween: 'in-between'
	};
	const numbersOf = $derived(
		new Map(
			cols
				.filter((c) => typeof c === 'number')
				.map((l) => [l, drawingNumbers(theCut.tracks[l] ?? [])])
		)
	);
	const nums = (layer: number) => numbersOf.get(layer) ?? new Map<number, number>();
	const layerName = (id: number) => layers.find((l) => l.id === id)?.name ?? '';

	function pick(c: Col, f: number) {
		picked = true;
		commitNote();
		typed = '';
		col = c;
		frame = f;
		if (typeof c === 'number') onlayer(c);
		onedit(false);
		sheetEl?.focus();
	}

	function move(dc: number, df: number) {
		const i = Math.max(0, Math.min(cols.length - 1, cols.indexOf(col) + dc));
		pick(cols[i], Math.max(0, Math.min(shown - 1, frame + df)));
	}

	let message = $state('');

	/** Write the typed number (or an empty cel) on the picked layer cell. */
	function commitNumber(n: number | 'empty'): boolean {
		if (typeof col !== 'number') return false;
		const layer = col;
		const t = trackIn(theCut, layer);
		let drawing = 0;
		if (n !== 'empty') {
			const known = nums(layer);
			drawing = [...known].find(([, k]) => k === n)?.[0] ?? 0;
			if (!drawing) {
				// the next number is a new, empty drawing; others don't exist (yet)
				if (n !== known.size + 1) {
					message = known.size
						? `${layerName(layer)} has drawings 1–${known.size} here. Type ${known.size + 1} for a new one.`
						: `Type 1 for ${layerName(layer)}'s first drawing.`;
					return false;
				}
				drawing = newDrawing();
				if (!drawing) return false;
			}
		}
		message = '';
		if (statics.includes(layer)) {
			if (drawing) setStaticDrawing(t, drawing);
		} else setKey(t, frame, drawing);
		onedit(true);
		return true;
	}

	function commitNote() {
		if (writing === null || typeof col === 'number') return;
		setNote(theCut, col, frame, note);
		writing = null;
		onedit(true);
	}

	function keydown(e: KeyboardEvent) {
		// the sheet's keys are its own (not the studio's shortcuts)
		if (writing !== null) {
			if (e.key === 'Enter') {
				e.preventDefault();
				commitNote();
				move(0, 1);
			} else if (e.key === 'Escape') {
				writing = null;
				sheetEl?.focus();
			}
			e.stopPropagation();
			return;
		}
		if (e.ctrlKey || e.metaKey || e.altKey) return;
		const nav: Record<string, [number, number]> = {
			ArrowUp: [0, -1],
			ArrowDown: [0, 1],
			ArrowLeft: [-1, 0],
			ArrowRight: [1, 0]
		};
		if (e.key === 'Enter' || e.key === 'ArrowDown') {
			e.preventDefault();
			e.stopPropagation();
			if (typed) {
				if (!commitNumber(+typed)) return;
				typed = '';
			} else if (e.key === 'Enter' && typeof col !== 'number') {
				startNote('');
				return;
			}
			move(0, 1);
			return;
		}
		if (nav[e.key]) {
			e.preventDefault();
			e.stopPropagation();
			typed = '';
			move(...nav[e.key]);
			return;
		}
		if (typeof col === 'number') {
			if (/^[0-9]$/.test(e.key)) {
				e.preventDefault();
				e.stopPropagation();
				typed = (typed + e.key).replace(/^0+/, '').slice(0, 4);
				if (e.key === '0' && !typed) {
					// 0 on its own: an empty cel
					if (commitNumber('empty')) move(0, 1);
				}
				return;
			}
			if (e.key === 'Backspace' && typed) {
				e.preventDefault();
				e.stopPropagation();
				typed = typed.slice(0, -1);
				return;
			}
			if (e.key === 'x' || e.key === 'X' || e.key === 'Delete' || e.key === 'Backspace') {
				e.preventDefault();
				e.stopPropagation();
				typed = '';
				if (commitNumber('empty')) move(0, 1);
				return;
			}
			if (e.key === 'Escape' && typed) {
				e.stopPropagation();
				typed = '';
			}
			return;
		}
		// a notes column: typing starts a note; Delete clears it
		if (e.key === 'Delete' || e.key === 'Backspace') {
			e.preventDefault();
			e.stopPropagation();
			setNote(theCut, col, frame, '');
			onedit(true);
			return;
		}
		if (e.key.length === 1) {
			e.preventDefault();
			e.stopPropagation();
			startNote((theCut[col]?.[frame] ?? '') + e.key);
		}
	}

	function startNote(text: string) {
		if (typeof col === 'number') return;
		note = text;
		writing = text;
	}
	function focusInput(el: HTMLInputElement) {
		el.focus();
		el.setSelectionRange(el.value.length, el.value.length);
	}

	// the current frame stays in view
	$effect(() => {
		const f = frame;
		sheetEl?.querySelector(`[data-frame="${f}"]`)?.scrollIntoView({ block: 'nearest' });
	});
	// another cut or scene: a fresh start
	$effect(() => {
		void [scene, cut];
		typed = '';
		writing = null;
		message = '';
	});
</script>

<section class="xsheet" aria-label="Exposure sheet">
	<p class="head">
		<b>{theScene.name} · {theCut.name}</b>
		<span>{length} frames</span>
	</p>
	<p class="help">
		Pick a cell and type a drawing number (Enter goes down), X for an empty cel. Type notes in
		Action and Dialogue.
	</p>
	{#if message}<p class="msg" role="status">{message}</p>{/if}
	<div
		class="sheet"
		bind:this={sheetEl}
		role="grid"
		tabindex="0"
		aria-label="Exposure sheet: {theCut.name}"
		onkeydown={keydown}
	>
		<table>
			<thead>
				<tr>
					<th class="fno" scope="col">Fr</th>
					<th class="txt" scope="col">Action</th>
					<th class="txt" scope="col">Dialogue</th>
					{#each cols.filter((c) => typeof c === 'number') as l (l)}
						<th
							scope="col"
							class="lay"
							class:on={l === active}
							title={layerName(l)}
							onclick={() => pick(l, frame)}
						>
							{layerName(l)}
						</th>
					{/each}
				</tr>
			</thead>
			<tbody>
				{#each Array.from({ length: shown }, (_, i) => i) as f (f)}
					<tr
						data-frame={f}
						class:now={f === frame}
						class:past={f >= length}
						class:sec={(f + 1) % fps === 0}
						class:half={fps % 2 === 0 && (f + 1) % (fps / 2) === 0}
					>
						<th class="fno" scope="row">
							{f + 1}{#if f && f % fps === 0}<small>{f / fps}s</small>{/if}
						</th>
						{#each ['action', 'dialogue'] as const as key (key)}
							{@const sel = col === key && f === frame}
							<td
								class="txt"
								role="gridcell"
								aria-selected={sel}
								aria-label="{key === 'action' ? 'Action' : 'Dialogue'}, frame {f + 1}"
								onclick={() => pick(key, f)}
								ondblclick={() => {
									pick(key, f);
									startNote(theCut[key]?.[f] ?? '');
								}}
							>
								{#if sel && writing !== null}
									<input
										class="note"
										aria-label="{key === 'action' ? 'Action' : 'Dialogue'} note"
										bind:value={note}
										use:focusInput
										onkeydown={keydown}
										onblur={commitNote}
									/>
								{:else}
									{theCut[key]?.[f] ?? ''}
								{/if}
							</td>
						{/each}
						{#each cols.filter((c) => typeof c === 'number') as l (l)}
							{@const c = cell(l, f)}
							{@const sel = col === l && f === frame}
							<td
								class="cel {c.kind}"
								data-mark={c.mark}
								role="gridcell"
								aria-selected={sel}
								aria-label="{layerName(l)}, frame {f + 1}: {c.kind === 'key'
									? `drawing ${c.text}${c.mark ? ` (${MARK_NAME[c.mark]})` : ''}`
									: c.kind === 'hold'
										? 'held'
										: c.kind === 'empty'
											? 'empty cel'
											: 'nothing'}"
								onclick={() => pick(l, f)}
							>
								{sel && typed ? typed : c.text}
							</td>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</section>

<style>
	.xsheet {
		display: flex;
		flex-direction: column;
		gap: 6px;
		height: 100%;
		min-height: 0;
		font: 13px var(--pp-font-body);
	}
	.head,
	.help,
	.msg {
		margin: 0;
	}
	.head {
		display: flex;
		justify-content: space-between;
	}
	.help {
		color: var(--pp-muted);
		font-size: 12px;
	}
	.msg {
		color: var(--pp-error);
		font-size: 12px;
	}
	.sheet {
		flex: 1;
		min-height: 200px;
		max-height: 70vh;
		overflow: auto;
		border-radius: 12px;
		background: var(--pp-paper, #fffdf6);
		box-shadow: inset 0 0 0 1px color-mix(in srgb, currentColor 12%, transparent);
		outline: none;
	}
	.sheet:focus-visible {
		box-shadow: inset 0 0 0 2px var(--pp-accent-strong, #e8492f);
	}
	table {
		border-collapse: collapse;
		width: max-content;
		min-width: 100%;
		table-layout: fixed;
	}
	th,
	td {
		height: 20px;
		padding: 0 4px;
		border-right: 1px solid color-mix(in srgb, currentColor 10%, transparent);
		border-bottom: 1px solid color-mix(in srgb, currentColor 6%, transparent);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	thead th {
		position: sticky;
		top: 0;
		z-index: 2;
		height: 26px;
		background: var(--pp-surface-sunk);
		font: 700 11px var(--pp-font-body);
		text-align: left;
	}
	.fno {
		position: sticky;
		left: 0;
		z-index: 1;
		width: 44px;
		background: var(--pp-ground);
		font: 600 11px var(--pp-font-body);
		color: var(--pp-ink-2);
		text-align: right;
	}
	thead .fno {
		z-index: 3;
	}
	.fno small {
		margin-left: 3px;
		color: var(--pp-accent-strong, #e8492f);
	}
	.txt {
		width: 84px;
		max-width: 84px;
		font-size: 12px;
	}
	.lay,
	.cel {
		width: 40px;
		max-width: 40px;
		text-align: center;
	}
	.lay {
		cursor: pointer;
	}
	.lay.on {
		background: #ffe9a8;
		color: #16161a;
	}
	td {
		cursor: cell;
	}
	.cel.key {
		font-weight: 800;
	}
	/* as on a paper sheet: keys circled, breakdowns boxed, in-betweens underlined */
	.cel[data-mark='key'],
	.cel[data-mark='breakdown'],
	.cel[data-mark='inbetween'] {
		background-clip: padding-box;
	}
	.cel[data-mark='key'] {
		background-image: radial-gradient(
			circle,
			transparent 0 8px,
			var(--pp-ink, var(--pp-ink)) 8px 9.5px,
			transparent 9.5px
		);
	}
	.cel[data-mark='breakdown'] {
		outline: 1.5px solid #6b5bd6;
		outline-offset: -4px;
	}
	.cel[data-mark='inbetween'] {
		text-decoration: underline;
	}
	.cel.empty {
		color: #9a9aa3;
	}
	/* a held drawing: a line down the middle of the cell */
	.cel.hold {
		background: linear-gradient(
			90deg,
			transparent calc(50% - 1px),
			var(--pp-ink) calc(50% - 1px),
			var(--pp-ink) calc(50% + 1px),
			transparent calc(50% + 1px)
		);
	}
	tr.past td {
		background-color: rgba(0, 0, 0, 0.025);
	}
	tr.half td,
	tr.half th {
		border-bottom: 1px solid color-mix(in srgb, currentColor 22%, transparent);
	}
	tr.sec td,
	tr.sec th {
		border-bottom: 2px solid var(--pp-ink);
	}
	tr.now th {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	tr.now td {
		background-color: rgba(255, 214, 80, 0.18);
	}
	td[aria-selected='true'] {
		box-shadow: inset 0 0 0 2px var(--pp-accent-strong, #e8492f);
	}
	.note {
		width: 100%;
		height: 18px;
		padding: 0 2px;
		border: 0;
		background: var(--pp-surface);
		font: inherit;
	}
</style>
