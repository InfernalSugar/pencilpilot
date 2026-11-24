<script lang="ts">
	/** Plays the drawing's timelapse, and saves it as a video. */
	import { onMount, untrack } from 'svelte';
	import { filmFps, recordFilm, videoType, type Timelapse } from './timelapse';

	let {
		film,
		onclose,
		onsave
	}: {
		film: Timelapse;
		onclose: () => void;
		/** saves the video some other way (the app: its own encoder), at this speed */
		onsave?: (fps: number, frames: number) => void;
	} = $props();

	let canvas = $state<HTMLCanvasElement>();
	let count = $state(untrack(() => film.length));
	let at = $state(0);
	let playing = $state(true);
	let fps = $state(untrack(() => filmFps(film.length)));
	let saving = $state<number | null>(null);
	let note = $state('');
	const canSave = untrack(() => !!onsave) || videoType() !== null;
	let dialog = $state<HTMLDialogElement>();

	function draw(i: number) {
		const f = film.frames[i];
		const g = canvas?.getContext('2d');
		if (!f || !g || !canvas) return;
		if (canvas.width !== f.width || canvas.height !== f.height) {
			canvas.width = f.width;
			canvas.height = f.height;
		}
		g.fillStyle = '#fff';
		g.fillRect(0, 0, f.width, f.height);
		g.drawImage(f, 0, 0);
	}

	onMount(() => {
		dialog?.showModal();
		const off = film.onchange(() => (count = film.length));
		let last = performance.now();
		let raf = requestAnimationFrame(function tick(now) {
			if (playing && count > 0) {
				if (at < count - 1 && now - last >= 1000 / fps) {
					at++;
					last = now;
				} else if (at >= count - 1 && now - last >= 1500) {
					// held on the finished drawing for a moment: start over
					at = 0;
					last = now;
				}
			}
			raf = requestAnimationFrame(tick);
		});
		return () => {
			off();
			cancelAnimationFrame(raf);
		};
	});

	$effect(() => draw(Math.min(at, count - 1)));

	async function save() {
		playing = false;
		if (onsave) {
			onsave(fps, count);
			return;
		}
		note = '';
		saving = 0;
		try {
			const { blob, ext } = await recordFilm(film.frames.slice(), fps, (p) => (saving = p));
			const a = document.createElement('a');
			a.href = URL.createObjectURL(blob);
			a.download = `pencilpilot-timelapse.${ext}`;
			a.click();
			setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
			note = `Saved (${ext.toUpperCase()}).`;
		} catch (e) {
			note = (e as Error).message;
		} finally {
			saving = null;
		}
	}
</script>

<dialog bind:this={dialog} class="player" aria-label="Timelapse" {onclose} oncancel={onclose}>
	<header>
		<h2>Timelapse</h2>
		<button type="button" class="x" aria-label="Close" onclick={() => dialog?.close()}>×</button>
	</header>
	{#if count === 0}
		<p class="empty">Draw something first: every change you make is a frame.</p>
	{:else}
		<canvas bind:this={canvas} data-testid="timelapse-canvas" data-frame={at}></canvas>
		<div class="controls">
			<button
				type="button"
				class="tool"
				aria-label={playing ? 'Pause' : 'Play'}
				onclick={() => (playing = !playing)}
			>
				{playing ? 'Pause' : 'Play'}
			</button>
			<label class="scrub">
				<span class="sr">Frame</span>
				<input
					type="range"
					min="0"
					max={Math.max(0, count - 1)}
					value={Math.min(at, count - 1)}
					oninput={(e) => {
						playing = false;
						at = +e.currentTarget.value;
					}}
				/>
			</label>
			<span class="count">{Math.min(at, count - 1) + 1} / {count}</span>
			<label class="speed">
				Speed
				<select bind:value={fps}>
					{#each [4, 8, 12, 20, 30] as f (f)}
						<option value={f}>{f} fps</option>
					{/each}
				</select>
			</label>
			<button
				type="button"
				class="tool done"
				disabled={!canSave || saving !== null}
				title={canSave ? 'Save the timelapse as a video' : 'This browser can’t record video'}
				onclick={save}
			>
				{saving !== null ? `Recording… ${Math.round(saving * 100)}%` : 'Save video'}
			</button>
		</div>
		{#if note}<p class="note" role="status">{note}</p>{/if}
	{/if}
</dialog>

<style>
	.player {
		width: min(860px, calc(100vw - 32px));
		padding: 16px;
		border: 0;
		border-radius: 20px;
		background: #1c1c22;
		color: #fff;
	}
	.player::backdrop {
		background: rgba(0, 0, 0, 0.55);
	}
	header {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}
	h2 {
		margin: 0;
		font-size: 20px;
	}
	.x {
		width: 44px;
		height: 44px;
		border: 0;
		border-radius: 50%;
		background: #2c2c35;
		color: #fff;
		font-size: 24px;
		cursor: pointer;
	}
	canvas {
		display: block;
		width: 100%;
		max-height: 62vh;
		margin: 12px 0;
		object-fit: contain;
		border-radius: 12px;
		background: #fff;
	}
	.controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
	}
	.scrub {
		flex: 1;
		min-width: 160px;
	}
	.scrub input {
		width: 100%;
	}
	.count {
		font-variant-numeric: tabular-nums;
		color: #c8c8d0;
	}
	.tool {
		min-height: 44px;
		padding: 0 16px;
		border: 0;
		border-radius: 12px;
		background: #2c2c35;
		color: #fff;
		font: 600 15px var(--pp-font-body, sans-serif);
		cursor: pointer;
	}
	.done {
		background: var(--pp-accent, #2f6bff);
	}
	.tool:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.speed select {
		min-height: 36px;
		margin-left: 6px;
		border-radius: 8px;
	}
	.empty,
	.note {
		color: #c8c8d0;
	}
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
