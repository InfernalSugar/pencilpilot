<script lang="ts">
	/**
	 * Export the animation: the format, which frames (this cut, the loop range, the
	 * scene, everything), the size, the frame rate, transparency. Shows progress while
	 * the studio renders and writes the frames; Cancel takes the file away.
	 */
	import { FORMATS, outputSize, type ExportSettings } from './exporting';

	let {
		settings = $bindable(),
		/** video formats ffmpeg can make here (none: ffmpeg isn't installed) */
		video,
		ffmpeg,
		fps,
		/** a loop range is set */
		hasLoop,
		/** the current cut's camera, in canvas pixels */
		camera,
		/** frames rendered so far, while exporting */
		progress,
		/** the project's layers are in stages (offer to export by stage) */
		staged = false,
		/** the canvas's view by stage ("All", "Rough"…) */
		viewName = 'All',
		onstart,
		oncancel,
		onclose
	}: {
		settings: ExportSettings;
		video: string[];
		ffmpeg: string | null;
		fps: number;
		hasLoop: boolean;
		camera: { width: number; height: number };
		progress: { done: number; total: number } | null;
		staged?: boolean;
		viewName?: string;
		onstart: () => void;
		oncancel: () => void;
		onclose: () => void;
	} = $props();

	const fmt = $derived(FORMATS.find((f) => f.id === settings.format) ?? FORMATS[0]);
	const usable = (id: string) => {
		const f = FORMATS.find((x) => x.id === id)!;
		return !f.video || video.includes(id);
	};
	const size = $derived(
		outputSize(
			{ x: 0, y: 0, ...camera },
			settings.scale,
			FORMATS.find((f) => f.id === settings.format)!.video
		)
	);
	// a format that can't be made here: the first one that can
	$effect(() => {
		if (!usable(settings.format)) settings.format = FORMATS.find((f) => usable(f.id))!.id;
	});
	let dialog = $state<HTMLDialogElement>();
	$effect(() => {
		if (dialog && !dialog.open) dialog.showModal();
	});
</script>

<dialog
	bind:this={dialog}
	class="export-dialog"
	aria-label="Export the animation"
	oncancel={(e) => {
		e.preventDefault();
		if (progress) oncancel();
		else onclose();
	}}
>
	<h2>Export the animation</h2>
	<div class="formats" role="radiogroup" aria-label="Format">
		{#each FORMATS as f (f.id)}
			<button
				type="button"
				role="radio"
				aria-checked={settings.format === f.id}
				disabled={!usable(f.id) || !!progress}
				title={usable(f.id) ? f.note : 'Needs ffmpeg (see below)'}
				onclick={() => (settings.format = f.id)}
			>
				<b>{f.label}</b><span>{f.note}</span>
			</button>
		{/each}
	</div>
	{#if !ffmpeg}
		<p class="note">
			MP4, ProRes and WebM need <b>ffmpeg</b>. Install it (for example from ffmpeg.org, or with your
			package manager) and open this again. GIF, animated PNG and PNG sequences work without it.
		</p>
	{/if}
	<div class="grid">
		<label>
			<span>Frames</span>
			<select bind:value={settings.range} disabled={!!progress} aria-label="Frames">
				<option value="cut">This cut</option>
				<option value="loop" disabled={!hasLoop}>The loop range (In / Out)</option>
				<option value="scene">This scene (every cut)</option>
				<option value="all">Everything</option>
			</select>
		</label>
		{#if staged}
			<label>
				<span>Show</span>
				<select bind:value={settings.show} disabled={!!progress} aria-label="Show">
					<option value="final">Final (no storyboard, rough or tie-down)</option>
					<option value="view">What the canvas shows ({viewName})</option>
					<option value="all">Every layer</option>
					<option value="each">Each stage, a file of its own</option>
				</select>
			</label>
		{/if}
		<label>
			<span>Size</span>
			<select bind:value={settings.scale} disabled={!!progress} aria-label="Size">
				<option value={1}>Full (100%)</option>
				<option value={0.75}>75%</option>
				<option value={0.5}>Half (50%)</option>
				<option value={0.25}>Quarter (25%)</option>
			</select>
		</label>
		<label>
			<span>Frame rate</span>
			<select bind:value={settings.fps} disabled={!!progress} aria-label="Frame rate">
				<option value={0}>The project's ({fps} fps)</option>
				{#each [12, 15, 24, 25, 30, 60].filter((n) => n !== fps) as n (n)}
					<option value={n}>{n} fps</option>
				{/each}
			</select>
		</label>
		<p class="size">{size.width} × {size.height} px, the cut's camera</p>
	</div>
	<div class="checks">
		<label class:off={!fmt.alpha}>
			<input
				type="checkbox"
				bind:checked={settings.transparent}
				disabled={!fmt.alpha || !!progress}
			/>
			Transparent background (no paper)
		</label>
		{#if settings.format === 'gif' || settings.format === 'apng'}
			<label>
				<input type="checkbox" bind:checked={settings.once} disabled={!!progress} />
				Play once (don't loop)
			</label>
		{/if}
	</div>
	{#if progress}
		<div class="progress" role="status" aria-live="polite">
			<progress max={progress.total} value={progress.done}></progress>
			<span>Frame {progress.done} of {progress.total}</span>
		</div>
	{/if}
	<div class="actions">
		{#if progress}
			<button type="button" onclick={oncancel}>Cancel</button>
		{:else}
			<button type="button" onclick={onclose}>Close</button>
			<button type="button" class="go" disabled={!usable(settings.format)} onclick={onstart}>
				Export…
			</button>
		{/if}
	</div>
</dialog>

<style>
	.export-dialog {
		width: min(560px, calc(100vw - 32px));
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.export-dialog::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 12px;
		font: 700 20px var(--pp-font-display);
	}
	.formats {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
		gap: 8px;
	}
	.formats button {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		padding: 10px 12px;
		border: 2px solid transparent;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.formats button span {
		font-size: 12px;
		color: var(--pp-muted);
	}
	.formats button[aria-checked='true'] {
		border-color: var(--pp-accent-strong, #e8492f);
		background: var(--pp-surface);
	}
	.formats button:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	.note {
		margin: 10px 0 0;
		padding: 8px 10px;
		border-radius: 10px;
		background: var(--pp-note);
		font-size: 13px;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 10px;
		margin-top: 14px;
	}
	.grid label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 12px;
		font-weight: 600;
	}
	.grid select {
		height: 34px;
		border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
		border-radius: 10px;
		background: var(--pp-surface);
		font: inherit;
	}
	.size {
		grid-column: 1 / -1;
		margin: 0;
		color: var(--pp-muted);
		font-size: 12px;
	}
	.checks {
		display: flex;
		flex-wrap: wrap;
		gap: 16px;
		margin-top: 12px;
	}
	.checks label.off {
		opacity: 0.45;
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
		border: 0;
		border-radius: 12px;
		background: var(--pp-track);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.actions .go {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.actions button:disabled {
		opacity: 0.45;
	}
	@media (max-width: 520px) {
		.grid {
			grid-template-columns: 1fr;
		}
	}
</style>
