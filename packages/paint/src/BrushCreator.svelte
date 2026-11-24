<script lang="ts">
	/**
	 * Make a brush of your own: a window over the canvas with the brush's settings
	 * in groups, a sample stroke that redraws as they change, and a little canvas to
	 * try it on. Nothing changes in My brushes until Save.
	 */
	import { onMount, untrack } from 'svelte';
	import BrushPreview from './BrushPreview.svelte';
	import BrushTestPad from './BrushTestPad.svelte';
	import TexturePicker from './TexturePicker.svelte';
	import { brushSource, type BrushManifest } from './brushes';
	import {
		brushTexture,
		DEFAULT_STRANDS,
		NO_TEXTURE,
		setBrushTexture,
		STRANDS_MAX,
		STRANDS_SPREAD,
		type BrushStrands,
		type BrushTexture
	} from './brush-textures';
	import {
		applyTweaks,
		changedTweaks,
		customBrush,
		deleteCustomBrush,
		isCustom,
		NEW_BRUSH_BASE,
		saveCustomBrush,
		SIZE_RANGE,
		tweaksOf,
		type BrushTweaks,
		type Tweaks
	} from './custom-brushes';

	let {
		manifest,
		start,
		askToDelete = true,
		ondone,
		onimport
	}: {
		manifest: BrushManifest;
		/** the brush to start from: one of My brushes (edit it), a built-in one, or none (a new brush) */
		start: string | null;
		/** ask "Delete this brush?" first (the app's Settings can switch it off) */
		askToDelete?: boolean;
		/** closed: the brush to use now (the one saved), or null to keep the one in use */
		ondone: (selected: string | null) => void;
		/** brush files chosen to import (the Brushes panel brings them in): how it went */
		onimport?: (files: File[]) => Promise<string>;
	} = $props();

	// (read once: the window edits the brush it was opened on)
	const editing = untrack(() => (start && isCustom(start) ? customBrush(start) : undefined));
	const base = untrack(() => editing?.base ?? start ?? NEW_BRUSH_BASE);
	const baseName = untrack(() => manifest.brushes.find((b) => b.id === base)?.name ?? 'Brush');
	const fresh = untrack(() => !start);

	let source = $state('');
	/** the brush as it came (Start over goes back to it) */
	let original: Tweaks | null = null;
	/** the settings when the window opened */
	let opened: Tweaks | null = null;
	/** tweaks the brush was saved with (Start over drops them) */
	let kept: BrushTweaks = untrack(() => editing?.tweaks ?? {});
	let t = $state<Tweaks | null>(null);
	let tex = $state<BrushTexture>(
		untrack(() => (fresh ? { ...NO_TEXTURE } : brushTexture(editing?.id ?? base)))
	);
	let name = $state(untrack(() => editing?.name ?? (fresh ? 'My brush' : `My ${baseName}`)));
	let error = $state('');
	let confirmDelete = $state(false);
	let testColor = $state('#16161a');
	let importing = $state('');

	const title = fresh
		? 'New brush'
		: editing
			? `Edit “${editing.name}”`
			: `Make “${baseName}” your own`;

	onMount(async () => {
		if (dialog && !dialog.open) {
			dialog.showModal();
			dialog.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
		}
		try {
			source = editing?.source ?? (await brushSource(base));
			original = tweaksOf(source);
			opened = editing ? tweaksOf(applyTweaks(source, kept)) : original;
			t = { ...opened };
		} catch {
			error = 'This brush couldn’t be loaded.';
		}
	});

	/** what Save keeps */
	const tweaks = (): BrushTweaks => (t && opened ? { ...kept, ...changedTweaks(opened, t) } : kept);
	/** the brush as set now, as the canvas will load it */
	const myb = $derived(t && source ? applyTweaks(source, tweaks()) : '');

	function save(copy: boolean) {
		if (!t) return;
		let n = name.trim() || 'My brush';
		if (copy && editing && n === editing.name) n = `${n} copy`;
		const b = saveCustomBrush({
			id: copy ? undefined : editing?.id,
			name: n,
			base,
			tweaks: tweaks(),
			source: editing?.source
		});
		setBrushTexture(b.id, tex);
		ondone(b.id);
	}

	function startOver() {
		if (!original) return;
		kept = {};
		opened = original;
		t = { ...original };
	}

	function remove() {
		if (!editing) return;
		deleteCustomBrush(editing.id);
		setBrushTexture(editing.id, null);
		ondone(editing.base);
	}

	// ---- the settings ----

	type Num = Exclude<keyof Tweaks, 'followDirection'>;
	interface Slider {
		key: Num;
		label: string;
		left: string;
		right: string;
		min: number;
		max: number;
		step: number;
		/** the value as shown */
		show?: (v: number) => string;
		/** a slider over the logarithm (sizes) */
		log?: boolean;
		hint?: string;
	}
	const pct = (v: number) => `${Math.round(v * 100)}%`;
	const GROUPS: {
		id: string;
		label: string;
		rows: (Slider | 'follow' | 'textures' | 'strands')[];
	}[] = [
		{
			id: 'shape',
			label: 'Shape',
			rows: [
				{
					key: 'size',
					label: 'Size',
					left: 'Fine',
					right: 'Big',
					min: SIZE_RANGE[0],
					max: SIZE_RANGE[1],
					step: 0.01,
					log: true,
					show: (v) => `${v < 10 ? v.toFixed(1) : Math.round(v)} px`,
					hint: 'Its size at 100% on the size slider.'
				},
				{
					key: 'sizeRandom',
					label: 'Size wobble',
					left: 'Even',
					right: 'Uneven',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'hardness',
					label: 'Edge',
					left: 'Soft',
					right: 'Hard',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'roundness',
					label: 'Tip',
					left: 'Round',
					right: 'Flat',
					min: 1,
					max: 10,
					step: 0.1,
					show: (v) => (v < 1.05 ? 'Round' : `${v.toFixed(1)}× as wide`)
				},
				{
					key: 'angle',
					label: 'Tip angle',
					left: '0°',
					right: '180°',
					min: 0,
					max: 180,
					step: 1,
					show: (v) => `${Math.round(v)}°`,
					hint: 'Turns a flat tip.'
				},
				'follow'
			]
		},
		{
			id: 'stroke',
			label: 'Stroke',
			rows: [
				{
					key: 'spacing',
					label: 'Spacing',
					left: 'Dotted',
					right: 'Smooth',
					min: 0.5,
					max: 10,
					step: 0.1,
					show: (v) => v.toFixed(1)
				},
				{
					key: 'smoothing',
					label: 'Smoothing',
					left: 'Off',
					right: 'Lots',
					min: 0,
					max: 10,
					step: 0.1,
					show: (v) => v.toFixed(1),
					hint: 'The line trails a little behind the pen, so it comes out smoother.'
				},
				{
					key: 'jitter',
					label: 'Scatter',
					left: 'Clean',
					right: 'Scattered',
					min: 0,
					max: 1.5,
					step: 0.01,
					show: pct
				},
				{
					key: 'shake',
					label: 'Shaky line',
					left: 'Steady',
					right: 'Shaky',
					min: 0,
					max: 2,
					step: 0.01,
					show: pct
				}
			]
		},
		{
			id: 'feel',
			label: 'Pressure and speed',
			rows: [
				{
					key: 'pressureSize',
					label: 'Press harder → thicker',
					left: 'Not at all',
					right: 'A lot',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'pressureOpacity',
					label: 'Press harder → darker',
					left: 'Not at all',
					right: 'A lot',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'speedSize',
					label: 'Draw faster →',
					left: 'Thinner',
					right: 'Thicker',
					min: -1,
					max: 1,
					step: 0.01,
					show: (v) => (Math.abs(v) < 0.005 ? 'No change' : pct(Math.abs(v)))
				},
				{
					key: 'tiltSize',
					label: 'Lean the pen → wider',
					left: 'Not at all',
					right: 'A lot',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct,
					hint: 'For pens that tell how far they lean.'
				}
			]
		},
		{
			id: 'colour',
			label: 'Colour',
			rows: [
				{
					key: 'opacity',
					label: 'Opacity',
					left: 'See-through',
					right: 'Solid',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct,
					hint: 'Each dab, at full pressure. Dabs overlap, so lines build up darker.'
				},
				{
					key: 'smudge',
					label: 'Blend with the paint underneath',
					left: 'None',
					right: 'All blend',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'smudgeLength',
					label: 'Carry blended paint',
					left: 'A little way',
					right: 'Far',
					min: 0,
					max: 1,
					step: 0.01,
					show: pct
				},
				{
					key: 'hueJitter',
					label: 'Colour wobble',
					left: 'None',
					right: 'Lots',
					min: 0,
					max: 0.5,
					step: 0.005,
					show: (v) => pct(v * 2),
					hint: 'Each dab a slightly different colour: try a bright test colour.'
				},
				{
					key: 'shadeJitter',
					label: 'Light and dark wobble',
					left: 'None',
					right: 'Lots',
					min: 0,
					max: 0.5,
					step: 0.005,
					show: (v) => pct(v * 2)
				}
			]
		},
		{ id: 'grain', label: 'Grain and tip picture', rows: ['textures'] },
		{ id: 'strands', label: 'Strands', rows: ['strands'] }
	];

	// ---- strands: one stroke, a bundle of thin lines ----

	interface StrandSlider {
		key: keyof BrushStrands;
		label: string;
		left: string;
		right: string;
		min: number;
		max: number;
		step: number;
		show: (v: number) => string;
		hint?: string;
	}
	const STRAND_SLIDERS: StrandSlider[] = [
		{
			key: 'count',
			label: 'Strands',
			left: 'Few',
			right: 'Many',
			min: 2,
			max: STRANDS_MAX,
			step: 1,
			show: (v) => `${Math.round(v)}`,
			hint: 'More strands take longer to draw.'
		},
		{
			key: 'spread',
			label: 'Spread',
			left: 'Narrow',
			right: 'Wide',
			min: STRANDS_SPREAD[0],
			max: STRANDS_SPREAD[1],
			step: 0.5,
			show: (v) => `${v.toFixed(1)}× the size`,
			hint: 'How wide the bundle is. Pressing harder spreads it more.'
		},
		{
			key: 'wobble',
			label: 'Wobble',
			left: 'Neat',
			right: 'Loose',
			min: 0,
			max: 1,
			step: 0.01,
			show: pct
		},
		{
			key: 'taper',
			label: 'Taper',
			left: 'Blunt',
			right: 'Pointed',
			min: 0,
			max: 1,
			step: 0.01,
			show: pct,
			hint: 'Each strand starts and ends thinner, at its own place.'
		}
	];
	/** the strand settings shown (the last ones used while they're off) */
	let strandsShown = $state<BrushStrands>(
		untrack(() => (tex.strands ? { ...tex.strands } : { ...DEFAULT_STRANDS }))
	);
	const strandsOnNow = $derived(!!tex.strands);
	function setStrands(on: boolean, patch: Partial<BrushStrands> = {}) {
		strandsShown = { ...strandsShown, ...patch };
		tex = { ...tex, strands: on ? { ...strandsShown } : null };
	}
	let group = $state('shape');
	const shown = $derived(GROUPS.find((g) => g.id === group)!);

	const toSlider = (s: Slider, v: number) => (s.log ? Math.log(v) : v);
	const fromSlider = (s: Slider, v: number) => (s.log ? Math.exp(v) : v);

	// ---- the window ----

	let dialog = $state<HTMLDialogElement>();
	let downOutside = false;
	let files = $state<HTMLInputElement>();
</script>

<dialog
	bind:this={dialog}
	class="creator"
	aria-labelledby="brush-creator-h"
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
			<h1 id="brush-creator-h">{title}</h1>
			<label class="name">
				<span>Name</span>
				<input type="text" maxlength="40" bind:value={name} aria-label="Brush name" />
			</label>
			{#if onimport}
				<button type="button" class="ghost" onclick={() => files?.click()}>
					Import brushes…
				</button>
				<input
					bind:this={files}
					class="hidden"
					type="file"
					multiple
					accept=".myb,.abr,.brush,.brushset,image/png,image/jpeg,image/webp"
					aria-label="Brush files to import"
					onchange={(e) => {
						const list = [...(e.currentTarget.files ?? [])];
						e.currentTarget.value = '';
						if (!list.length) return;
						importing = 'Reading…';
						void onimport(list).then((note) => (importing = note));
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

		{#if importing}<p class="note" role="status">{importing}</p>{/if}
		{#if error}
			<p class="error" role="alert">{error}</p>
		{:else}
			<div class="split">
				<div class="groups" role="tablist" aria-label="Brush settings" aria-orientation="vertical">
					{#each GROUPS as g (g.id)}
						<button
							type="button"
							role="tab"
							id="brush-tab-{g.id}"
							aria-selected={group === g.id}
							aria-controls="brush-settings"
							onclick={() => (group = g.id)}
						>
							{g.label}
						</button>
					{/each}
				</div>

				<div
					class="settings"
					id="brush-settings"
					role="tabpanel"
					aria-labelledby="brush-tab-{group}"
				>
					<h2>{shown.label}</h2>
					{#if t}
						{#each shown.rows as row (typeof row === 'string' ? row : row.key)}
							{#if row === 'follow'}
								<label class="check">
									<input type="checkbox" bind:checked={t.followDirection} />
									<span>Tip turns with the stroke <small>(like a calligraphy pen)</small></span>
								</label>
							{:else if row === 'textures'}
								<TexturePicker value={tex} onchange={(v) => (tex = v)} />
							{:else if row === 'strands'}
								<p class="hint intro">
									One stroke draws a bundle of thin lines that follow it: quick hair, fur and grass.
								</p>
								<label class="check">
									<input
										type="checkbox"
										checked={strandsOnNow}
										onchange={(e) => setStrands(e.currentTarget.checked)}
									/>
									<span>Draw in strands</span>
								</label>
								{#each STRAND_SLIDERS as s (s.key)}
									<label class="row" class:off={!strandsOnNow}>
										<span class="lbl">
											{s.label}
											<b>{s.show(strandsShown[s.key])}</b>
										</span>
										<span class="ends">
											<span>{s.left}</span>
											<input
												type="range"
												min={s.min}
												max={s.max}
												step={s.step}
												value={strandsShown[s.key]}
												disabled={!strandsOnNow}
												aria-label="Strand {s.label.toLowerCase()}"
												oninput={(e) => setStrands(true, { [s.key]: +e.currentTarget.value })}
											/>
											<span>{s.right}</span>
										</span>
										{#if s.hint}<small>{s.hint}</small>{/if}
									</label>
								{/each}
							{:else}
								{@const s = row}
								<label class="row">
									<span class="lbl">
										{s.label}
										<b>{s.show ? s.show(t[s.key]) : t[s.key]}</b>
									</span>
									<span class="ends">
										<span>{s.left}</span>
										<input
											type="range"
											min={toSlider(s, s.min)}
											max={toSlider(s, s.max)}
											step={s.log ? 0.01 : s.step}
											value={toSlider(s, t[s.key])}
											aria-label={s.label}
											oninput={(e) => t && (t[s.key] = fromSlider(s, +e.currentTarget.value))}
										/>
										<span>{s.right}</span>
									</span>
									{#if s.hint}<small>{s.hint}</small>{/if}
								</label>
							{/if}
						{/each}
					{:else}
						<p class="hint">Loading the brush…</p>
					{/if}
				</div>

				<div class="try">
					<h2>Preview</h2>
					{#if myb}
						<BrushPreview
							{myb}
							texture={tex}
							color={testColor}
							width={480}
							height={120}
							label="Sample stroke with these settings: light, firm, then light"
						/>
					{/if}
					<p class="hint">Light pressure → firm → light.</p>
					<div class="try-head">
						<h2>Try it</h2>
						<label class="colour">
							<span>Colour</span>
							<input type="color" bind:value={testColor} aria-label="Colour to try it with" />
						</label>
					</div>
					{#if myb}
						<BrushTestPad {myb} texture={tex} color={testColor} width={480} height={220} />
					{/if}
					<p class="hint">Your next stroke uses the brush as it’s set now.</p>
				</div>
			</div>

			<footer>
				{#if editing}
					{#if confirmDelete}
						<span class="warn">Delete this brush?</span>
						<button type="button" class="ghost danger" onclick={remove}>Delete</button>
						<button type="button" class="ghost" onclick={() => (confirmDelete = false)}>
							Keep
						</button>
					{:else}
						<button
							type="button"
							class="ghost"
							onclick={() => (askToDelete ? (confirmDelete = true) : remove())}
						>
							Delete brush
						</button>
					{/if}
				{/if}
				<button type="button" class="ghost" onclick={startOver} disabled={!t}>
					{editing || fresh ? 'Start over' : `Back to ${baseName}`}
				</button>
				<span class="space"></span>
				<button type="button" class="ghost" onclick={() => ondone(null)}>Cancel</button>
				{#if editing}
					<button type="button" class="ghost" onclick={() => save(true)} disabled={!t}>
						Save as a copy
					</button>
				{/if}
				<button type="button" class="primary" onclick={() => save(false)} disabled={!t}>
					{editing ? 'Save' : 'Save to My brushes'}
				</button>
			</footer>
		{/if}
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
		margin-top: 8px;
	}
	.try-head h2 {
		margin: 0;
	}
	.colour {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.colour input {
		width: 40px;
		height: 30px;
		padding: 2px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 8px;
		background: var(--pp-ground);
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin: 0 0 14px;
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
	.ends {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
		color: var(--pp-ink-2);
	}
	.ends span {
		flex: 0 0 5.6em;
	}
	.ends span:last-child {
		text-align: right;
	}
	.ends input {
		flex: 1;
		min-width: 0;
		accent-color: var(--pp-accent-strong);
	}
	.check {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 4px 0 12px;
		font-size: 13px;
		font-weight: 600;
	}
	.check input {
		accent-color: var(--pp-accent-strong);
	}
	.row.off {
		opacity: 0.5;
	}
	.intro {
		margin: 0 0 10px;
	}
	small,
	.hint {
		margin: 0;
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
	.warn,
	.error {
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
