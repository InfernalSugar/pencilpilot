<script lang="ts">
	import { onMount } from 'svelte';
	import { Replayer, findMoments, timeline, type Timeline } from '../replay';
	import { toGray } from '../grader';
	import type { Submission } from '../submission';
	import type { Stroke } from '../paint/strokes';

	let {
		sub,
		tips = [],
		firedAt
	}: {
		sub: Submission;
		/** the results page's tips, to mark on the timeline */
		tips?: { id: string; title: string }[];
		/** which tips a partial drawing gets (for the moments) */
		firedAt?: (gray: Uint8Array, strokes: Stroke[]) => Promise<string[]>;
	} = $props();

	let el: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;
	let r: Replayer | undefined;
	let tl = $state<Timeline | null>(null);
	let error = $state('');
	let playing = $state(false);
	let speed = $state(2);
	/** replay clock, ms */
	let at = $state(0);
	/** what has been drawn so far: stroke index and next point index */
	let cursor = { s: 0, i: 0 };
	let moments = $state<Record<string, number> | null>(null);
	let momentsBusy = $state(false);
	/** a tip whose moment was just passed (shown briefly) */
	let flash = $state<{ id: string; title: string } | null>(null);
	let flashTimer: ReturnType<typeof setTimeout> | undefined;
	let raf = 0;

	const fmt = (ms: number) => {
		const s = Math.round(ms / 1000);
		return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
	};

	function paint(full = false) {
		if (!r || !ctx) return;
		const rect = r.flush();
		if (!rect && !full) return;
		const data = new Uint8ClampedArray(r.memory.buffer, r.paint.rgbaPtr(), r.width * r.height * 4);
		const img = new ImageData(data, r.width, r.height);
		if (full || !rect) ctx.putImageData(img, 0, 0);
		else ctx.putImageData(img, 0, 0, ...rect);
	}

	/** draw everything up to replay time `t` (from the current cursor on) */
	function advance(t: number) {
		if (!r || !tl) return;
		while (cursor.s < tl.strokes.length) {
			const s = tl.strokes[cursor.s];
			const ts = tl.times[cursor.s];
			if (ts[cursor.i] > t) break;
			if (cursor.i === 0 && !r.begin(s)) {
				cursor = { s: cursor.s + 1, i: 0 };
				continue;
			}
			while (cursor.i < s.points.length && ts[cursor.i] <= t) r.point(s, cursor.i++);
			if (cursor.i < s.points.length) break;
			r.end(s);
			cursor = { s: cursor.s + 1, i: 0 };
		}
	}

	function seek(t: number) {
		if (!r || !tl) return;
		const before = at;
		at = Math.max(0, Math.min(tl.duration, t));
		if (at < before) {
			// going back: start over and redraw up to there
			r.reset();
			cursor = { s: 0, i: 0 };
		}
		advance(at);
		paint(at < before);
	}

	function frame(now: number, last: number) {
		if (!playing || !tl) return;
		const prev = at;
		seek(at + (now - last) * speed);
		if (moments)
			for (const tip of tips) {
				const m = moments[tip.id];
				if (m !== undefined && prev < m && at >= m) {
					flash = tip;
					clearTimeout(flashTimer);
					flashTimer = setTimeout(() => (flash = null), 2600);
				}
			}
		if (at >= tl.duration) playing = false;
		else raf = requestAnimationFrame((n) => frame(n, now));
	}

	// ---- save the replay as a video: play it from the start and record the canvas ----
	let recording = $state<number | null>(null);
	let videoError = $state('');
	const VIDEO_TYPES = ['video/mp4;codecs=avc1', 'video/mp4', 'video/webm;codecs=vp9', 'video/webm'];
	const videoType = () =>
		typeof MediaRecorder === 'undefined'
			? null
			: (VIDEO_TYPES.find((t) => MediaRecorder.isTypeSupported(t)) ?? null);
	const canRecord = typeof window !== 'undefined' && !!videoType();

	async function saveVideo() {
		const type = videoType();
		if (!type || !tl || recording !== null) return;
		videoError = '';
		pause();
		// the whole drawing in about 15 s (at least 2x, at most 32x)
		const rate = Math.min(32, Math.max(2, tl.duration / 15_000));
		const stream = el.captureStream(30);
		const rec = new MediaRecorder(stream, { mimeType: type, videoBitsPerSecond: 6_000_000 });
		const chunks: Blob[] = [];
		rec.ondataavailable = (e) => {
			if (e.data.size) chunks.push(e.data);
		};
		const stopped = new Promise<void>((done) => (rec.onstop = () => done()));
		seek(0);
		paint(true);
		rec.start(250);
		recording = 0;
		await new Promise<void>((done) => {
			let last = performance.now();
			const step = (now: number) => {
				seek(at + (now - last) * rate);
				last = now;
				recording = Math.round((at / Math.max(1, tl!.duration)) * 100);
				if (at >= tl!.duration) done();
				else requestAnimationFrame(step);
			};
			requestAnimationFrame(step);
		});
		// hold the finished drawing for a moment
		await new Promise((r) => setTimeout(r, 1200));
		rec.stop();
		await stopped;
		stream.getTracks().forEach((t) => t.stop());
		recording = null;
		if (!chunks.length) {
			videoError = 'This browser couldn’t record the replay.';
			return;
		}
		const blob = new Blob(chunks, { type: type.split(';')[0] });
		const a = document.createElement('a');
		a.href = URL.createObjectURL(blob);
		const ext = type.includes('mp4') ? 'mp4' : 'webm';
		a.download = `pencilpilot-replay-${new Date().toISOString().slice(0, 10)}.${ext}`;
		a.click();
		setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
	}

	function play() {
		if (!tl) return;
		if (at >= tl.duration) seek(0);
		playing = true;
		const t0 = performance.now();
		raf = requestAnimationFrame((n) => frame(n, t0));
	}
	function pause() {
		playing = false;
		cancelAnimationFrame(raf);
	}

	onMount(() => {
		let dead = false;
		const t = timeline(sub.strokeData ?? []);
		if (!t.strokes.length) {
			error = 'This drawing has no recorded strokes to replay.';
			return;
		}
		Replayer.create(sub.width, sub.height, sub.layers, t.strokes)
			.then(async (rep) => {
				if (dead) return rep.free();
				r = rep;
				ctx = el.getContext('2d');
				tl = t;
				paint(true);
				play();
				if (firedAt && tips.length) {
					// a second engine finds the moments, so playback isn't disturbed
					momentsBusy = true;
					const probe = await Replayer.create(sub.width, sub.height, sub.layers, t.strokes);
					try {
						const found = await findMoments(
							t,
							probe,
							tips.map((x) => x.id),
							firedAt,
							toGray
						);
						if (!dead) moments = found;
					} finally {
						probe.free();
						momentsBusy = false;
					}
				}
			})
			.catch((e) => (error = `Couldn’t start the replay: ${e}`));
		return () => {
			dead = true;
			cancelAnimationFrame(raf);
			clearTimeout(flashTimer);
			r?.free();
		};
	});
</script>

<div class="replay">
	<div
		class="frame"
		style="aspect-ratio: {sub.width} / {sub.height}; --a: {sub.width / sub.height}"
	>
		<canvas
			bind:this={el}
			width={sub.width}
			height={sub.height}
			aria-label="Replay of your drawing"
			data-ready={!!tl}
		></canvas>
		{#if flash}
			<p class="flash" role="status">{flash.title}</p>
		{/if}
		{#if error}<p class="msg" role="alert">{error}</p>{/if}
	</div>
	{#if tl}
		<div class="controls">
			<button
				type="button"
				class="play"
				aria-label={playing ? 'Pause' : 'Play'}
				onclick={() => (playing ? pause() : play())}
			>
				{#if playing}
					<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14M16 5v14" /></svg>
				{:else}
					<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 4l13 8-13 8Z" /></svg>
				{/if}
			</button>
			<div class="track">
				<input
					type="range"
					min="0"
					max={Math.max(1, Math.round(tl.duration))}
					value={Math.round(at)}
					aria-label="Replay position"
					aria-valuetext="{fmt(at)} of {fmt(tl.duration)}"
					oninput={(e) => {
						pause();
						seek(+e.currentTarget.value);
					}}
				/>
				{#if moments}
					{#each tips as tip (tip.id)}
						{#if moments[tip.id] !== undefined}
							<button
								type="button"
								class="mark"
								style="left: {(moments[tip.id] / Math.max(1, tl.duration)) * 100}%"
								title={tip.title}
								aria-label="Jump to: {tip.title}"
								onclick={() => {
									pause();
									seek(moments![tip.id]);
								}}
							></button>
						{/if}
					{/each}
				{/if}
			</div>
			<span class="time">{fmt(at)} / {fmt(tl.duration)}</span>
			{#if canRecord}
				<button
					type="button"
					class="save"
					disabled={recording !== null}
					onclick={saveVideo}
					title="Save the replay as a video"
				>
					{recording !== null ? `Recording… ${recording}%` : 'Save video'}
				</button>
			{/if}
			<label class="speed">
				<span class="visually-hidden">Speed</span>
				<select bind:value={speed} aria-label="Replay speed">
					{#each [1, 2, 4, 8] as x (x)}<option value={x}>{x}×</option>{/each}
				</select>
			</label>
		</div>
		{#if videoError}<p class="note" role="alert">{videoError}</p>{/if}
		{#if momentsBusy}
			<p class="note">Finding when each tip happened…</p>
		{:else if moments && tips.length}
			<p class="note">Marks on the timeline show where each tip’s mistake started.</p>
		{/if}
	{/if}
</div>

<style>
	.replay {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.frame {
		position: relative;
		width: min(100%, calc(70vh * var(--a, 1.33)));
		margin-inline: auto;
		border-radius: 18px;
		overflow: hidden;
		border: 1px solid var(--pp-track);
		background: #fff;
	}
	canvas {
		display: block;
		width: 100%;
		height: 100%;
	}
	.flash {
		position: absolute;
		left: 50%;
		bottom: 14px;
		transform: translateX(-50%);
		margin: 0;
		padding: 8px 14px;
		max-width: calc(100% - 28px);
		border-radius: 999px;
		background: var(--pp-accent-strong);
		color: #fff;
		font-weight: 600;
		font-size: 14px;
		text-align: center;
		animation: pop 0.25s ease-out;
	}
	@keyframes pop {
		from {
			transform: translate(-50%, 8px);
			opacity: 0;
		}
	}
	.msg {
		position: absolute;
		inset: 0;
		margin: 0;
		display: grid;
		place-items: center;
		padding: 24px;
		color: var(--pp-muted);
		font-weight: 600;
		text-align: center;
	}
	.controls {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.play {
		flex: none;
		display: grid;
		place-items: center;
		width: 44px;
		height: 44px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		cursor: pointer;
	}
	svg {
		width: 18px;
		height: 18px;
		fill: currentColor;
		stroke: currentColor;
		stroke-width: 2.4;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.track {
		position: relative;
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
	}
	.track input {
		width: 100%;
		accent-color: var(--pp-accent);
	}
	.mark {
		position: absolute;
		top: -14px;
		width: 14px;
		height: 14px;
		margin-left: -7px;
		padding: 0;
		border: 2px solid #fff;
		border-radius: 999px;
		background: var(--pp-accent-strong);
		cursor: pointer;
	}
	.time {
		flex: none;
		font-size: 13px;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
		color: var(--pp-ink-2);
	}
	.save {
		flex: none;
		min-height: 36px;
		padding: 0 12px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
		white-space: nowrap;
	}
	.save:disabled {
		cursor: progress;
		color: var(--pp-ink-2);
	}
	select {
		min-height: 36px;
		border: 1px solid var(--pp-track);
		border-radius: 10px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
	}
	.note {
		margin: 0;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
