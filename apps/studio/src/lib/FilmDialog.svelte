<script lang="ts">
	/**
	 * Export a film of the drawing being made: the project's timelapse, or this frame
	 * drawn live, stroke by stroke. MP4 / WebM need ffmpeg (bundled); GIF doesn't.
	 */
	import { FILM_FPS, type FilmSettings } from './film';

	let {
		settings = $bindable(),
		/** video formats ffmpeg can make here */
		video,
		/** pictures in the timelapse */
		pictures,
		/** strokes in the frame showing, and how long they took to draw (s, as drawn) */
		strokes,
		drawnSeconds,
		/** the output size (drawn live: the cut's camera at the scale) */
		size,
		progress,
		onstart,
		oncancel,
		onclose
	}: {
		settings: FilmSettings;
		video: string[];
		pictures: number;
		strokes: number;
		drawnSeconds: number;
		size: { width: number; height: number };
		progress: { done: number; total: number } | null;
		onstart: () => void;
		oncancel: () => void;
		onclose: () => void;
	} = $props();

	let el = $state<HTMLDialogElement>();
	$effect(() => {
		if (el && !el.open) el.showModal();
	});
	const busy = $derived(!!progress);
	const usable = (f: string) => f === 'gif' || video.includes(f);
	$effect(() => {
		if (!usable(settings.format)) settings.format = 'gif';
	});
	const empty = $derived(settings.kind === 'timelapse' ? pictures === 0 : strokes === 0);
	/** about how long the film runs, with its ending */
	const length = $derived(
		settings.kind === 'timelapse'
			? settings.seconds + 1.5
			: drawnSeconds / settings.speed + 0.5 + 0.6 + 1.5
	);
	const time = (s: number) =>
		s < 60 ? `${Math.round(s)} s` : `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`;
</script>

<dialog
	bind:this={el}
	class="film-dialog"
	aria-label="Export a film of the drawing"
	oncancel={(e) => {
		e.preventDefault();
		if (busy) oncancel();
		else onclose();
	}}
>
	<h2>Film of the drawing</h2>
	<div class="kinds" role="radiogroup" aria-label="Film">
		<button
			type="button"
			role="radio"
			aria-checked={settings.kind === 'timelapse'}
			disabled={busy}
			onclick={() => (settings.kind = 'timelapse')}
		>
			<b>Timelapse</b>
			<span>The whole project, a picture after every change ({pictures} so far)</span>
		</button>
		<button
			type="button"
			role="radio"
			aria-checked={settings.kind === 'live'}
			disabled={busy}
			onclick={() => (settings.kind = 'live')}
		>
			<b>Drawn live</b>
			<span>This frame, stroke by stroke at drawing speed ({strokes} strokes)</span>
		</button>
	</div>

	<div class="grid">
		<label>
			<span>Format</span>
			<select bind:value={settings.format} disabled={busy} aria-label="Format">
				<option value="mp4" disabled={!usable('mp4')}>MP4 video</option>
				<option value="webm" disabled={!usable('webm')}>WebM video</option>
				<option value="gif">GIF</option>
			</select>
		</label>
		{#if settings.kind === 'timelapse'}
			<label>
				<span>Length</span>
				<select bind:value={settings.seconds} disabled={busy} aria-label="Length">
					{#each [10, 15, 30, 60] as s (s)}
						<option value={s}>{s} seconds</option>
					{/each}
				</select>
			</label>
		{:else}
			<label>
				<span>Speed</span>
				<select bind:value={settings.speed} disabled={busy} aria-label="Speed">
					<option value={1}>As drawn</option>
					<option value={2}>2× faster</option>
					<option value={4}>4× faster</option>
					<option value={8}>8× faster</option>
				</select>
			</label>
			<label>
				<span>Size</span>
				<select bind:value={settings.scale} disabled={busy} aria-label="Size">
					<option value={1}>Full (100%)</option>
					<option value={0.5}>Half (50%)</option>
				</select>
			</label>
		{/if}
	</div>
	<p class="about">
		{size.width} × {size.height} px, {FILM_FPS} fps, about {time(length)}
		{#if settings.kind === 'live'}
			— lines draw on, then the finished frame (fills and all) eases in.
		{:else}
			— ends on the finished picture.
		{/if}
	</p>
	{#if empty}
		<p class="note">
			{settings.kind === 'timelapse'
				? 'Nothing in the timelapse yet: it takes a picture after each change.'
				: 'No strokes on this frame’s drawings yet.'}
		</p>
	{/if}
	{#if progress}
		<div class="progress" role="status" aria-live="polite">
			<progress max={progress.total} value={progress.done}></progress>
			<span>Frame {progress.done} of {progress.total}</span>
		</div>
	{/if}
	<div class="actions">
		{#if busy}
			<button type="button" onclick={oncancel}>Cancel</button>
		{:else}
			<button type="button" onclick={onclose}>Close</button>
			<button
				type="button"
				class="go"
				disabled={empty || !usable(settings.format)}
				onclick={onstart}
			>
				Export…
			</button>
		{/if}
	</div>
</dialog>

<style>
	.film-dialog {
		width: min(560px, calc(100vw - 32px));
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.film-dialog::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 12px;
		font: 700 20px var(--pp-font-display);
	}
	.kinds {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 8px;
	}
	.kinds button {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		padding: 10px 12px;
		border: 2px solid transparent;
		border-radius: 12px;
		background: #f1ecdf;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.kinds span {
		font-size: 12px;
		color: #6b6b74;
	}
	.kinds button[aria-checked='true'] {
		border-color: var(--pp-accent-strong, #e8492f);
		background: #fff;
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
		border: 1px solid rgba(0, 0, 0, 0.15);
		border-radius: 10px;
		background: #fff;
		font: inherit;
	}
	.about {
		margin: 10px 0 0;
		color: #6b6b74;
		font-size: 12px;
	}
	.note {
		margin: 10px 0 0;
		padding: 8px 10px;
		border-radius: 10px;
		background: #fff4d6;
		font-size: 13px;
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
		background: #e9e4d6;
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
		.grid,
		.kinds {
			grid-template-columns: 1fr;
		}
	}
</style>
