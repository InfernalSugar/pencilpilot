<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import BrushPreview from './BrushPreview.svelte';
	import { brushSource, type BrushManifest } from './brushes';
	import {
		applyTweaks,
		customBrush,
		deleteCustomBrush,
		saveCustomBrush,
		tweaksOf,
		type BrushTweaks
	} from './custom-brushes';

	let {
		manifest,
		selected = $bindable(),
		onclose
	}: { manifest: BrushManifest; selected: string; onclose: () => void } = $props();

	// what we're editing: one of "My brushes", or a built-in brush to start a new one from
	// (read once: the editor edits the brush that was selected when it opened)
	const editing = untrack(() => customBrush(selected));
	const base = untrack(() => editing?.base ?? selected);
	const baseName = untrack(() => manifest.brushes.find((b) => b.id === base)?.name ?? 'Brush');

	let source = $state('');
	let start = $state<Required<BrushTweaks> | null>(null);
	let t = $state<Required<BrushTweaks>>({
		hardness: 0.8,
		spacing: 2,
		jitter: 0,
		pressureSize: 0.5,
		pressureOpacity: 0.5
	});
	let name = $state(editing?.name ?? `My ${baseName}`);
	let confirmDelete = $state(false);
	let error = $state('');

	onMount(async () => {
		try {
			source = await brushSource(base);
			start = tweaksOf(source);
			t = { ...start, ...(editing?.tweaks ?? {}) };
		} catch {
			error = 'This brush couldn’t be loaded.';
		}
	});

	const preview = $derived(source ? applyTweaks(source, t) : '');

	const SLIDERS: {
		key: keyof BrushTweaks;
		label: string;
		left: string;
		right: string;
		min: number;
		max: number;
		step: number;
	}[] = [
		{ key: 'hardness', label: 'Edge', left: 'Soft', right: 'Hard', min: 0, max: 1, step: 0.01 },
		{
			key: 'spacing',
			label: 'Spacing',
			left: 'Dotted',
			right: 'Smooth',
			min: 0.5,
			max: 10,
			step: 0.1
		},
		{
			key: 'jitter',
			label: 'Grain',
			left: 'Clean',
			right: 'Scattered',
			min: 0,
			max: 1.5,
			step: 0.01
		},
		{
			key: 'pressureSize',
			label: 'Press harder → thicker',
			left: 'Not at all',
			right: 'A lot',
			min: 0,
			max: 1,
			step: 0.01
		},
		{
			key: 'pressureOpacity',
			label: 'Press harder → darker',
			left: 'Not at all',
			right: 'A lot',
			min: 0,
			max: 1,
			step: 0.01
		}
	];

	function save(asNew: boolean) {
		const b = saveCustomBrush({
			id: asNew ? undefined : editing?.id,
			name,
			base,
			tweaks: { ...t }
		});
		selected = b.id;
		onclose();
	}

	function remove() {
		if (!editing) return;
		deleteCustomBrush(editing.id);
		selected = editing.base;
		onclose();
	}
</script>

<section class="editor" aria-label="Adjust brush">
	<div class="head">
		<b>{editing ? `Adjust “${editing.name}”` : `Make your own from “${baseName}”`}</b>
		<button type="button" class="ghost" onclick={onclose}>Cancel</button>
	</div>

	{#if error}
		<p class="error" role="alert">{error}</p>
	{:else if preview}
		<BrushPreview
			myb={preview}
			label="Sample stroke with these settings: light, firm, then light"
		/>
		<p class="hint">Sample stroke: light pressure → firm → light.</p>

		{#each SLIDERS as s (s.key)}
			<label class="row">
				<span class="lbl">{s.label}</span>
				<span class="ends">
					<span>{s.left}</span>
					<input
						type="range"
						min={s.min}
						max={s.max}
						step={s.step}
						bind:value={t[s.key]}
						aria-label={s.label}
					/>
					<span>{s.right}</span>
				</span>
			</label>
		{/each}

		<label class="row">
			<span class="lbl">Name</span>
			<input class="name" type="text" maxlength="40" bind:value={name} aria-label="Brush name" />
		</label>

		<div class="actions">
			{#if editing}
				<button type="button" class="primary" onclick={() => save(false)}>Save changes</button>
				<button type="button" class="ghost" onclick={() => save(true)}>Save as a new brush</button>
			{:else}
				<button type="button" class="primary" onclick={() => save(true)}>Save to My brushes</button>
			{/if}
			<button type="button" class="ghost" onclick={() => start && (t = { ...start })}>
				Back to {baseName}
			</button>
			{#if editing}
				{#if confirmDelete}
					<span class="warn">Delete this brush?</span>
					<button type="button" class="ghost danger" onclick={remove}>Delete</button>
					<button type="button" class="ghost" onclick={() => (confirmDelete = false)}>Keep</button>
				{:else}
					<button type="button" class="ghost" onclick={() => (confirmDelete = true)}>
						Delete brush
					</button>
				{/if}
			{/if}
		</div>
		<p class="hint">My brushes are kept in this browser on this device.</p>
	{/if}
</section>

<style>
	.editor {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
	}
	.head b {
		font: 700 15px var(--pp-font-display);
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.hint {
		margin: -4px 0 0;
		font-size: 12px;
		color: var(--pp-muted);
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.lbl {
		font-size: 13px;
		font-weight: 700;
	}
	.ends {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
		color: var(--pp-ink-2);
	}
	.ends span {
		flex: 0 0 4.8em;
	}
	.ends span:last-child {
		text-align: right;
	}
	.ends input {
		flex: 1;
		min-width: 0;
		accent-color: var(--pp-accent);
	}
	.name {
		padding: 8px 10px;
		border: 2px solid var(--pp-track);
		border-radius: 12px;
		font: 500 14px var(--pp-font-body);
		background: var(--pp-ground);
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.primary,
	.ghost {
		min-height: 36px;
		padding: 0 12px;
		border-radius: 12px;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.primary {
		border: 0;
		background: var(--pp-ink);
		color: #fff;
	}
	.ghost {
		border: 1px solid var(--pp-track);
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	.danger {
		border-color: var(--pp-accent-strong);
		color: var(--pp-accent-ink);
	}
	.warn,
	.error {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
</style>
