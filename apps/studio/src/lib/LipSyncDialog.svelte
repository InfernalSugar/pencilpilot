<script lang="ts">
	/**
	 * Lip-sync a sound clip: pick the mouth layer, say which of its drawings is each
	 * mouth shape (its mouth chart), the language and timing; Rhubarb listens and the
	 * mouths go onto the layer for the clip's frames.
	 */
	import { SHAPE_INFO, type LipSyncSettings } from './lipsync';
	import { MOUTH_SHAPES, type MouthChart, type MouthShape } from './timeline';

	let {
		name,
		/** the frames it will write, in this cut (1-based for people) */
		range,
		/** the clip runs past the cut's end: lengthen the cut to hold all of it */
		longer,
		toEnd = $bindable(),
		layers,
		layer = $bindable(),
		chart,
		choices,
		thumb,
		settings = $bindable(),
		dialog = $bindable(),
		/** where Rhubarb is (none: not installed) */
		rhubarb,
		/** 0–1 while listening */
		progress,
		error,
		onchart,
		onauto,
		onstart,
		oncancel,
		onclose
	}: {
		name: string;
		range: { from: number; frames: number };
		longer: boolean;
		toEnd: boolean;
		layers: { id: number; name: string }[];
		layer: number;
		chart: MouthChart;
		choices: { id: number; label: string }[];
		thumb: (drawing: number) => Promise<string | null>;
		settings: LipSyncSettings;
		dialog: string;
		rhubarb: string | null;
		progress: number | null;
		error: string;
		onchart: (shape: MouthShape, drawing: number) => void;
		onauto: () => void;
		onstart: () => void;
		oncancel: () => void;
		onclose: () => void;
	} = $props();

	let el = $state<HTMLDialogElement>();
	$effect(() => {
		if (el && !el.open) el.showModal();
	});
	const busy = $derived(progress !== null);
	const basic = $derived(MOUTH_SHAPES.filter((k) => !SHAPE_INFO[k].extended));
	const missing = $derived(basic.filter((k) => !chart.shapes[k]));
	const any = $derived(MOUTH_SHAPES.some((k) => chart.shapes[k]));

	// thumbnails of the chart's drawings (made once each)
	let thumbs = $state<Record<number, string | null>>({});
	$effect(() => {
		for (const k of MOUTH_SHAPES) {
			const d = chart.shapes[k];
			if (d && !(d in thumbs)) {
				thumbs[d] = null;
				void thumb(d).then((url) => (thumbs[d] = url));
			}
		}
	});
</script>

<dialog
	bind:this={el}
	class="lipsync-dialog"
	aria-label="Lip-sync"
	oncancel={(e) => {
		e.preventDefault();
		if (busy) oncancel();
		else onclose();
	}}
>
	<h2>Lip-sync “{name}”</h2>
	<p class="range">
		Frames {range.from + 1}–{range.from + range.frames} of this cut
		{#if longer}
			<label>
				<input type="checkbox" bind:checked={toEnd} disabled={busy} />
				Make the cut long enough for the whole line
			</label>
		{/if}
	</p>
	{#if !rhubarb}
		<p class="note">
			Lip-sync needs <b>Rhubarb Lip Sync</b>, which comes with the installed app. Running from
			source: <code>node scripts/fetch-rhubarb.mjs</code> in apps/studio.
		</p>
	{/if}

	<label class="field">
		<span>Mouth layer</span>
		<select bind:value={layer} disabled={busy} aria-label="Mouth layer">
			{#each layers as l (l.id)}
				<option value={l.id}>{l.name}</option>
			{/each}
		</select>
	</label>

	<div class="chart-head">
		<b>Mouth chart</b>
		<span>Which drawing is each mouth. G, H and X are optional.</span>
		<button type="button" onclick={onauto} disabled={busy || !choices.length}>Fill in order</button>
	</div>
	<div class="chart" role="group" aria-label="Mouth chart">
		{#each MOUTH_SHAPES as k (k)}
			{@const d = chart.shapes[k] ?? 0}
			<label class="shape" class:extended={SHAPE_INFO[k].extended} class:set={!!d}>
				<span class="pic">
					{#if d && thumbs[d]}
						<img src={thumbs[d]} alt="" />
					{:else}
						<b>{k}</b>
					{/if}
				</span>
				<span class="what"><b>{k}</b> {SHAPE_INFO[k].label}</span>
				<select
					aria-label="Mouth {k}"
					title={SHAPE_INFO[k].hint}
					value={d}
					disabled={busy}
					onchange={(e) => onchart(k, +(e.currentTarget as HTMLSelectElement).value)}
				>
					<option value={0}>—</option>
					{#each choices as c (c.id)}
						<option value={c.id}>{c.label}</option>
					{/each}
				</select>
			</label>
		{/each}
	</div>
	{#if !choices.length}
		<p class="note">
			Draw each mouth on its own drawing of this layer first (N makes a new drawing): A, B, C, D, E,
			F, then G, H and X if you like. “Fill in order” matches them up.
		</p>
	{:else if missing.length && any}
		<p class="hint">
			No drawing for {missing.join(', ')}: the nearest shape stands in.
		</p>
	{/if}

	<div class="grid">
		<label class="field">
			<span>Language</span>
			<select
				aria-label="Language"
				disabled={busy}
				value={settings.english ? 'en' : 'other'}
				onchange={(e) => (settings.english = (e.currentTarget as HTMLSelectElement).value === 'en')}
			>
				<option value="en">English (hears words)</option>
				<option value="other">Another language (hears sounds)</option>
			</select>
		</label>
		<label class="field">
			<span>Timing</span>
			<select bind:value={settings.step} disabled={busy} aria-label="Timing">
				<option value={1}>On ones</option>
				<option value={2}>On twos</option>
				<option value={3}>On threes</option>
			</select>
		</label>
		<label class="field">
			<span>Mouths lead the sound by</span>
			<select bind:value={settings.lead} disabled={busy} aria-label="Lead">
				<option value={0}>0 frames</option>
				<option value={1}>1 frame</option>
				<option value={2}>2 frames</option>
			</select>
		</label>
	</div>
	<label class="field">
		<span>What’s said (optional; helps with English)</span>
		<textarea
			rows="2"
			bind:value={dialog}
			disabled={busy || !settings.english}
			aria-label="What’s said"
			placeholder="The line, as written"></textarea>
	</label>

	{#if error}<p class="error" role="alert">{error}</p>{/if}
	{#if busy}
		<div class="progress" role="status" aria-live="polite">
			<progress max="1" value={progress}></progress>
			<span>Listening… {Math.round((progress ?? 0) * 100)}%</span>
		</div>
	{/if}
	<div class="actions">
		{#if busy}
			<button type="button" onclick={oncancel}>Cancel</button>
		{:else}
			<button type="button" onclick={onclose}>Close</button>
			<button type="button" class="go" disabled={!rhubarb || !any} onclick={onstart}>
				Lip-sync
			</button>
		{/if}
	</div>
</dialog>

<style>
	.lipsync-dialog {
		width: min(640px, calc(100vw - 32px));
		max-height: calc(100vh - 32px);
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.lipsync-dialog::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 4px;
		font: 700 20px var(--pp-font-display);
	}
	.range {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 16px;
		margin: 0 0 12px;
		color: #6b6b74;
		font-size: 13px;
	}
	.range label {
		color: var(--pp-ink, #16161a);
	}
	.note,
	.hint {
		margin: 8px 0;
		padding: 8px 10px;
		border-radius: 10px;
		background: #fff4d6;
		font-size: 13px;
	}
	.hint {
		background: #f1ecdf;
	}
	.error {
		margin: 8px 0;
		color: #b3261e;
		font-size: 13px;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
		margin-top: 10px;
		font-size: 12px;
		font-weight: 600;
	}
	select,
	textarea {
		border: 1px solid rgba(0, 0, 0, 0.15);
		border-radius: 10px;
		background: #fff;
		font: inherit;
		font-weight: 400;
	}
	select {
		height: 32px;
	}
	textarea {
		padding: 6px 8px;
		resize: vertical;
	}
	.chart-head {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 4px 10px;
		margin-top: 14px;
	}
	.chart-head span {
		flex: 1;
		color: #6b6b74;
		font-size: 12px;
	}
	.chart-head button,
	.actions button {
		height: 32px;
		padding: 0 12px;
		border: 0;
		border-radius: 10px;
		background: #e9e4d6;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.chart {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(104px, 1fr));
		gap: 8px;
		margin-top: 8px;
	}
	.shape {
		display: flex;
		flex-direction: column;
		align-items: stretch;
		gap: 4px;
		padding: 6px;
		border-radius: 12px;
		background: #f1ecdf;
		font-size: 11px;
	}
	.shape.extended {
		background: #f6f2e8;
	}
	.shape.set {
		background: #fff;
		box-shadow: inset 0 0 0 2px var(--pp-accent-strong, #e8492f);
	}
	.pic {
		display: grid;
		place-items: center;
		height: 56px;
		border-radius: 8px;
		background: #fff;
		color: #b8b2a3;
		font-size: 22px;
	}
	.pic img {
		max-width: 100%;
		max-height: 100%;
	}
	.what {
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 10px;
	}
	.progress {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-top: 14px;
	}
	.progress progress {
		flex: 1;
		accent-color: var(--pp-accent-strong, #e8492f);
	}
	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 16px;
	}
	.actions button {
		height: 38px;
		padding: 0 16px;
		font-size: 14px;
	}
	.actions .go {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.actions button:disabled,
	.chart-head button:disabled {
		opacity: 0.45;
	}
	@media (max-width: 560px) {
		.grid {
			grid-template-columns: 1fr;
		}
	}
</style>
