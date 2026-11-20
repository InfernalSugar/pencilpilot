<script lang="ts">
	import { onMount } from 'svelte';
	import { enhance } from '$app/forms';
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { imageUrl, titleOf, type Reference } from '#lib/references.js';
	import {
		cleanKeypoints,
		labelIdeas,
		MAX_KEYPOINTS,
		MIN_KEYPOINTS,
		type RefKeypoint
	} from '#lib/keypoints.js';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	let category = $state(page.url.searchParams.get('category') ?? 'all');
	let onlyUnmarked = $state(page.url.searchParams.has('unmarked'));
	let selectedId = $state<string | null>(page.url.searchParams.get('id'));
	let points = $state<RefKeypoint[]>([]);
	let nextLabel = $state('');
	let dirty = $state(false);
	let status = $state('');
	let frame = $state<HTMLDivElement>();
	let dragging = $state<number | null>(null);
	/** set once the page is interactive (tests wait for it) */
	let ready = $state(false);
	// Saving changes content/references/*.json, which makes Vite reload the page: the
	// URL keeps the reference and filters, and the last message is carried over.
	const STATUS = 'pp.keypointStatus';
	/** what a save leaves for the page after Vite's reload (kept for a few seconds,
	 * since the reload can come before the save's answer, or twice) */
	type Carry = { status: string; next?: string; at: number };
	function carry(c: Carry | null) {
		try {
			if (c) sessionStorage.setItem(STATUS, JSON.stringify(c));
			else sessionStorage.removeItem(STATUS);
		} catch {
			/* storage blocked */
		}
	}
	onMount(() => {
		try {
			const c: Carry | null = JSON.parse(sessionStorage.getItem(STATUS) ?? 'null');
			if (c && Date.now() - c.at < 10_000) {
				status = c.status;
				if (c.next && page.url.searchParams.get('id') !== c.next) setUrl({ id: c.next });
			}
		} catch {
			/* storage blocked or bad data */
		}
		ready = true;
	});

	function setUrl(change: Record<string, string | null>) {
		const u = new URL(page.url.href);
		for (const [k, v] of Object.entries(change))
			if (v === null) u.searchParams.delete(k);
			else u.searchParams.set(k, v);
		replaceState(u, page.state);
	}

	const marked = (r: Reference) => (cleanKeypoints(r.keypoints)?.length ?? 0) >= MIN_KEYPOINTS;
	const shown = $derived(
		data.refs.filter(
			(r) => (category === 'all' || r.category === category) && (!onlyUnmarked || !marked(r))
		)
	);
	const selected = $derived(data.refs.find((r) => r.id === selectedId) ?? null);
	const ideas = $derived(selected ? labelIdeas(selected.category, selected.topic) : []);
	const unused = $derived(ideas.filter((l) => !points.some((p) => p.label === l)));
	const markedCount = $derived(data.refs.filter(marked).length);

	/** Load the saved points of the selected reference (on select, and after a save). */
	$effect(() => {
		const r = selected;
		const saved = r ? (cleanKeypoints(r.keypoints) ?? []) : [];
		points = saved;
		dirty = false;
		nextLabel = r ? (labelIdeas(r.category, r.topic)[saved.length] ?? '') : '';
	});

	function select(id: string) {
		if (dirty && !confirmLeave()) return;
		selectedId = id;
		status = '';
		setUrl({ id });
	}

	/** Unsaved points: keep them unless the reviewer clicks again within a few seconds. */
	let warnedAt = 0;
	function confirmLeave() {
		if (Date.now() - warnedAt < 4000) return true;
		warnedAt = Date.now();
		status = 'Unsaved points. Click again to leave without saving.';
		return false;
	}

	function at(e: PointerEvent) {
		const b = frame!.getBoundingClientRect();
		const c = (v: number) => Math.min(1, Math.max(0, v));
		return { x: c((e.clientX - b.left) / b.width), y: c((e.clientY - b.top) / b.height) };
	}

	function down(e: PointerEvent) {
		if (!selected || e.button !== 0) return;
		const hit = (e.target as HTMLElement).closest<HTMLElement>('[data-point]');
		if (hit) {
			dragging = Number(hit.dataset.point);
			frame!.setPointerCapture(e.pointerId);
			return;
		}
		if (points.length >= MAX_KEYPOINTS) {
			status = `${MAX_KEYPOINTS} points at most.`;
			return;
		}
		const label = nextLabel.trim() || `point ${points.length + 1}`;
		points.push({ ...at(e), label });
		dirty = true;
		nextLabel = unused.find((l) => l !== label) ?? '';
	}

	function move(e: PointerEvent) {
		if (dragging === null) return;
		const p = at(e);
		points[dragging].x = p.x;
		points[dragging].y = p.y;
		dirty = true;
	}

	function swap(i: number, j: number) {
		if (j < 0 || j >= points.length) return;
		[points[i], points[j]] = [points[j], points[i]];
		dirty = true;
	}

	function remove(i: number) {
		points.splice(i, 1);
		dirty = true;
	}

	const pct = (v: number) => `${(v * 100).toFixed(2)}%`;
	const nextUnmarked = $derived(
		shown.find((r, i) => i > shown.findIndex((s) => s.id === selectedId) && !marked(r)) ??
			shown.find((r) => !marked(r) && r.id !== selectedId)
	);
	let goNext = false;
</script>

<svelte:head><title>Mark keypoints · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow"><a href="/dev">Dev tools</a> · M5-T03</span>
	<h1>Mark keypoints</h1>
	<p class="lede">
		After drawing a reference with keypoints, students tap the same spots on their drawing, and the
		grader measures their proportions from them. Pick {MIN_KEYPOINTS}–{MAX_KEYPOINTS} spots that are easy
		to find in any drawing (eyes, chin, paw tips, a roof peak) and spread out over the subject.
		<b>Order matters:</b> the angle from each point to the next is checked too, so go around the
		shape rather than jumping back and forth. Marked: <b>{markedCount}</b> of {data.refs.length}.
	</p>

	<div class="bar">
		<label>
			Category
			<select
				bind:value={category}
				onchange={() => setUrl({ category: category === 'all' ? null : category })}
			>
				<option value="all">All</option>
				{#each data.categories as c (c)}<option value={c}>{c}</option>{/each}
			</select>
		</label>
		<label class="check">
			<input
				type="checkbox"
				bind:checked={onlyUnmarked}
				onchange={() => setUrl({ unmarked: onlyUnmarked ? '1' : null })}
			/> Only unmarked
		</label>
	</div>

	<div class="layout">
		<ul class="list" aria-label="References">
			{#each shown as r (r.id)}
				{@const n = cleanKeypoints(r.keypoints)?.length ?? 0}
				<li>
					<button
						type="button"
						class="thumb"
						class:on={r.id === selectedId}
						aria-pressed={r.id === selectedId}
						aria-label="{titleOf(r)} ({r.id}), {n} points"
						onclick={() => select(r.id)}
					>
						<img src={imageUrl(r)} alt="" loading="lazy" />
						<span class="badge" class:done={n >= MIN_KEYPOINTS}>{n}</span>
					</button>
				</li>
			{:else}
				<li class="muted">Nothing here.</li>
			{/each}
		</ul>

		<section class="editor" aria-label="Editor">
			{#if !selected}
				<p class="muted">Pick a reference on the left.</p>
			{:else}
				<h2>{titleOf(selected)} <small>{selected.id} · {selected.category}</small></h2>
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div
					class="frame"
					bind:this={frame}
					data-testid="keypoint-frame"
					data-ready={ready}
					style="aspect-ratio: {selected.width} / {selected.height}; width: min(100%, calc(70vh * {selected.width /
						selected.height}))"
					onpointerdown={down}
					onpointermove={move}
					onpointerup={() => (dragging = null)}
					onpointercancel={() => (dragging = null)}
				>
					<img src={imageUrl(selected)} alt={titleOf(selected)} draggable="false" />
					<svg viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true">
						<polyline
							points={points.map((p) => `${p.x},${p.y}`).join(' ')}
							vector-effect="non-scaling-stroke"
						/>
					</svg>
					{#each points as p, i (i)}
						<span
							class="pt"
							class:drag={dragging === i}
							data-point={i}
							style="left: {pct(p.x)}; top: {pct(p.y)}"
							title={p.label}
						>
							{i + 1}
						</span>
					{/each}
				</div>

				<div class="next">
					<label>
						Next point
						<input
							bind:value={nextLabel}
							list="kp-ideas"
							placeholder="what is it? e.g. nose tip"
							aria-label="Label for the next point"
						/>
					</label>
					<datalist id="kp-ideas">
						{#each ideas as l (l)}<option value={l}></option>{/each}
					</datalist>
					<div class="chips">
						{#each unused as l (l)}
							<button
								type="button"
								class="chip"
								class:on={l === nextLabel}
								onclick={() => (nextLabel = l)}
							>
								{l}
							</button>
						{/each}
					</div>
				</div>

				<ol class="points" aria-label="Points">
					{#each points as p, i (i)}
						<li>
							<span class="n">{i + 1}</span>
							<input
								bind:value={p.label}
								oninput={() => (dirty = true)}
								aria-label="Point {i + 1} label"
							/>
							<button
								type="button"
								aria-label="Move point {i + 1} up"
								onclick={() => swap(i, i - 1)}
							>
								↑
							</button>
							<button
								type="button"
								aria-label="Move point {i + 1} down"
								onclick={() => swap(i, i + 1)}
							>
								↓
							</button>
							<button type="button" aria-label="Delete point {i + 1}" onclick={() => remove(i)}>
								✕
							</button>
						</li>
					{/each}
				</ol>
				{#if points.length && points.length < MIN_KEYPOINTS}
					<p class="warn">
						Students only get the step with {MIN_KEYPOINTS} or more points.
					</p>
				{/if}

				<form
					method="POST"
					action="?/save"
					use:enhance={() => {
						const next = goNext ? nextUnmarked?.id : undefined;
						const n = points.length;
						// stored before sending: Vite may reload as soon as the file is written
						carry({ status: `Saved ${n} points.`, next, at: Date.now() });
						return async ({ result, update }) => {
							if (result.type === 'success') {
								dirty = false;
								status = `Saved ${n} points.`;
								if (next) setUrl({ id: next });
							} else {
								carry(null);
								status = 'Not saved. Check the labels.';
							}
							await update({ reset: false });
							if (result.type === 'success' && next) select(next);
						};
					}}
				>
					<input type="hidden" name="id" value={selected.id} />
					<input type="hidden" name="category" value={selected.category} />
					<input type="hidden" name="keypoints" value={JSON.stringify(points)} />
					<div class="actions">
						<button class="ok" onclick={() => (goNext = false)}>Save</button>
						<button class="ok" onclick={() => (goNext = true)} disabled={!nextUnmarked}>
							Save &amp; next
						</button>
						<button
							type="button"
							class="no"
							disabled={!points.length}
							onclick={() => {
								points = [];
								dirty = true;
								nextLabel = ideas[0] ?? '';
							}}
						>
							Clear
						</button>
						<span role="status" class="status">{status || (form?.message ?? '')}</span>
					</div>
				</form>
			{/if}
		</section>
	</div>
</div>

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 8px;
	}
	h2 {
		font-size: 22px;
		margin: 0 0 8px;
	}
	h2 small {
		font: 400 13px var(--pp-font-body);
		color: var(--pp-muted);
	}
	.lede {
		color: var(--pp-ink-2);
		max-width: 900px;
		line-height: 1.6;
	}
	.muted {
		color: var(--pp-muted);
	}
	.bar {
		display: flex;
		gap: 16px;
		align-items: center;
		margin: 12px 0;
		flex-wrap: wrap;
	}
	.check {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.layout {
		display: grid;
		grid-template-columns: minmax(160px, 260px) 1fr;
		gap: 16px;
		align-items: start;
	}
	@media (max-width: 720px) {
		.layout {
			grid-template-columns: 1fr;
		}
		.list {
			max-height: 220px;
		}
	}
	.list {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
		gap: 6px;
		max-height: 75vh;
		overflow: auto;
	}
	.thumb {
		position: relative;
		width: 100%;
		aspect-ratio: 1;
		padding: 0;
		border: 3px solid transparent;
		border-radius: 12px;
		overflow: hidden;
		background: var(--pp-ground);
		cursor: pointer;
	}
	.thumb.on {
		border-color: var(--pp-accent);
	}
	.thumb img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.badge {
		position: absolute;
		right: 4px;
		bottom: 4px;
		min-width: 20px;
		padding: 0 5px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font: 700 12px/20px var(--pp-font-body);
	}
	.badge.done {
		background: var(--pp-good, #2f9e5b);
	}
	.editor {
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		padding: 16px;
		min-width: 0;
	}
	.frame {
		position: relative;
		margin-inline: auto;
		background: var(--pp-ground);
		border-radius: 12px;
		cursor: crosshair;
		touch-action: none;
		user-select: none;
	}
	.frame img,
	.frame svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: fill;
		border-radius: 12px;
	}
	.frame svg {
		pointer-events: none;
		overflow: visible;
	}
	polyline {
		fill: none;
		stroke: var(--pp-accent);
		stroke-width: 2;
		stroke-dasharray: 6 4;
	}
	.pt {
		position: absolute;
		width: 24px;
		height: 24px;
		margin: -12px 0 0 -12px;
		border-radius: 50%;
		background: var(--pp-accent);
		color: #fff;
		border: 2px solid #fff;
		box-shadow: 0 1px 4px rgb(0 0 0 / 0.4);
		font: 700 12px/20px var(--pp-font-body);
		text-align: center;
		cursor: grab;
	}
	.pt.drag {
		cursor: grabbing;
		transform: scale(1.2);
	}
	.next {
		margin-top: 12px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.next label {
		display: flex;
		gap: 8px;
		align-items: center;
	}
	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.chip {
		padding: 4px 10px;
		border-radius: 999px;
		border: 1px solid var(--pp-track);
		background: var(--pp-surface);
		font: 13px var(--pp-font-body);
		cursor: pointer;
	}
	.chip.on {
		background: var(--pp-ink);
		color: #fff;
	}
	.points {
		list-style: none;
		padding: 0;
		margin: 12px 0 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.points li {
		display: flex;
		gap: 4px;
		align-items: center;
	}
	.points input {
		flex: 1;
		min-width: 0;
	}
	.n {
		width: 24px;
		text-align: center;
		font-weight: 700;
	}
	.points button {
		padding: 4px 8px;
		border: 0;
		border-radius: 8px;
		background: var(--pp-ground);
		cursor: pointer;
	}
	.warn {
		color: var(--pp-accent-ink);
		font-size: 14px;
	}
	select,
	input {
		padding: 6px 8px;
		border-radius: 10px;
		border: 2px solid var(--pp-track);
		font: inherit;
		font-size: 14px;
	}
	.actions {
		display: flex;
		gap: 8px;
		align-items: center;
		flex-wrap: wrap;
		margin-top: 12px;
	}
	.actions button {
		padding: 8px 14px;
		border: 0;
		border-radius: 10px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.actions button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.ok {
		background: var(--pp-ink);
		color: #fff;
	}
	.no {
		background: var(--pp-ground);
		color: var(--pp-accent-ink);
	}
	.status {
		color: var(--pp-ink-2);
		font-size: 14px;
	}
</style>
