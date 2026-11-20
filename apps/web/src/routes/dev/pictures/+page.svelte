<script lang="ts">
	import { onMount, tick } from 'svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import { TIERS, tierName } from '#lib/references.js';
	import {
		DEFAULT_FILTERS,
		SOURCES,
		applyDecision,
		failure,
		keptPercent,
		keyAction,
		listQuery,
		moveOptions,
		reviewLabel,
		serialQueue,
		step,
		type Decision,
		type Filters,
		type Picture,
		type TopicStats
	} from '#lib/picture-review.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const catName = (id: string) => data.categories.find((c) => c.id === id)?.name ?? id;
	const topicLabel = (c: string, t: string) => data.topics[c]?.find((x) => x.id === t)?.name ?? t;

	let filters: Filters = $state({ ...DEFAULT_FILTERS });
	let items: Picture[] = $state([]);
	let total = $state(0);
	let page = $state(1);
	let seed = $state(Math.floor(Math.random() * 1e9));
	let loading = $state(false);
	let apiError = $state('');
	let message = $state('');
	let stats: TopicStats[] = $state([]);
	let cursor = $state(0);
	let focusMode = $state(false);
	let moveChoice: Record<string, string> = $state({});
	let pickerFor: Picture | null = $state(null);
	let pickerQuery = $state('');
	let picker: HTMLDialogElement | undefined = $state();
	let grid: HTMLElement | undefined = $state();

	interface UndoEntry {
		id: string;
		prev: Picture;
		/** the server's sequence number, once it has answered (0: nothing to undo) */
		seq: number;
	}
	const undoStack: UndoEntry[] = [];
	let undoCount = $state(0);
	const enqueue = serialQueue();

	const current = $derived(items[cursor]);
	const visibleStats = $derived(
		stats.filter(
			(s) =>
				(!filters.category || s.category === filters.category) &&
				(!filters.topic || s.topic === filters.topic)
		)
	);
	const statsTotal = $derived(
		visibleStats.reduce(
			(a, s) => ({
				pictures: a.pictures + s.pictures,
				reviewed: a.reviewed + s.reviewed,
				kept: a.kept + s.kept,
				rejected: a.rejected + s.rejected
			}),
			{ pictures: 0, reviewed: 0, kept: 0, rejected: 0 }
		)
	);
	const pickerOptions = $derived(
		pickerFor
			? moveOptions(pickerFor, data.topics).filter((o) =>
					`${catName(o.category)} ${o.name}`.toLowerCase().includes(pickerQuery.toLowerCase())
				)
			: []
	);

	async function api(path: string, init?: RequestInit) {
		return fetch(`/api/admin/${path}`, {
			...init,
			headers: init?.body ? { 'content-type': 'application/json' } : undefined
		});
	}

	async function loadPage(reset: boolean) {
		if (loading) return;
		loading = true;
		try {
			const next = reset ? 1 : page + 1;
			const res = await api(`pictures?${listQuery(filters, next, seed)}`);
			if (!res.ok) {
				apiError = await failure(res);
				return;
			}
			apiError = '';
			const body = await res.json();
			items = reset ? body.items : [...items, ...body.items];
			total = body.total;
			page = next;
			if (reset) cursor = 0;
		} catch (e) {
			apiError = `Can't reach the ref-service (${e}). Is pnpm dev running?`;
		} finally {
			loading = false;
		}
	}

	async function loadStats() {
		try {
			const res = await api('stats');
			if (res.ok) stats = (await res.json()).topics;
		} catch {
			/* the list shows the error */
		}
	}

	function setFilter<K extends keyof Filters>(k: K, v: Filters[K]) {
		filters[k] = v;
		if (k === 'category') filters.topic = '';
		if (k === 'sort' && v === 'random') seed = Math.floor(Math.random() * 1e9);
		void loadPage(true);
	}

	const replace = (id: string, p: Picture) => {
		const i = items.findIndex((x) => x.id === id);
		if (i >= 0) items[i] = p;
	};

	function decide(
		p: Picture,
		decision: Decision,
		target: { category?: string; topic?: string; tier?: number } = {}
	) {
		if (p.rejected) {
			message = 'Already rejected here. Press Z to undo it.';
			return;
		}
		const prev = $state.snapshot(p) as Picture;
		replace(p.id, applyDecision(prev, decision, target));
		const entry: UndoEntry = { id: p.id, prev, seq: 0 };
		undoStack.push(entry);
		undoCount = undoStack.length;
		message = '';
		void enqueue(async () => {
			let res: Response;
			try {
				res = await api('review', {
					method: 'POST',
					body: JSON.stringify({ id: p.id, decision, ...target })
				});
			} catch (e) {
				return fail(entry, `${decision} ${p.id}: ${e}`);
			}
			if (!res.ok) return fail(entry, `${decision} ${p.id}: ${await failure(res)}`);
			const body = await res.json();
			if (body.unchanged || !body.seq) {
				drop(entry);
			} else {
				entry.seq = body.seq;
			}
			if (body.picture) replace(p.id, body.picture);
			void loadStats();
		});
		if (focusMode || decision !== 'tier') advance();
	}

	function drop(entry: UndoEntry) {
		const i = undoStack.indexOf(entry);
		if (i >= 0) undoStack.splice(i, 1);
		undoCount = undoStack.length;
	}

	function fail(entry: UndoEntry, why: string) {
		replace(entry.id, entry.prev);
		drop(entry);
		message = `Not saved: ${why}`;
	}

	function undo() {
		const entry = undoStack.pop();
		undoCount = undoStack.length;
		if (!entry) {
			message = 'Nothing to undo.';
			return;
		}
		const now = items.find((x) => x.id === entry.id);
		replace(entry.id, entry.prev);
		const i = items.findIndex((x) => x.id === entry.id);
		if (i >= 0) cursor = i;
		message = `Undid ${entry.id}.`;
		void enqueue(async () => {
			// the decision itself failed or changed nothing: nothing on the server to undo
			if (!entry.seq) return;
			try {
				const res = await api('undo', {
					method: 'POST',
					body: JSON.stringify({ seq: entry.seq })
				});
				if (!res.ok) throw new Error(await failure(res));
				const body = await res.json();
				if (body.picture) replace(entry.id, body.picture);
				void loadStats();
			} catch (e) {
				if (now) replace(entry.id, now);
				message = `Undo failed: ${e instanceof Error ? e.message : e}`;
			}
		});
	}

	function advance() {
		if (cursor < items.length - 1) cursor += 1;
		if (cursor >= items.length - 6 && items.length < total) void loadPage(false);
	}

	async function openPicker(p: Picture | undefined) {
		if (!p || p.rejected) return;
		pickerFor = p;
		pickerQuery = '';
		await tick();
		picker?.showModal();
	}

	function pick(o: { category: string; topic: string }) {
		const p = pickerFor;
		picker?.close();
		if (p) decide(p, 'move', { category: o.category, topic: o.topic });
	}

	function moveFromCard(p: Picture) {
		const v = moveChoice[p.id];
		if (!v) {
			message = 'Pick a topic to move it to first.';
			return;
		}
		const [category, topic] = v.split('/');
		decide(p, 'move', { category, topic });
	}

	function onKey(e: KeyboardEvent) {
		if (pickerFor && picker?.open) return;
		const t = e.target as HTMLElement | null;
		if (t && ['INPUT', 'SELECT', 'TEXTAREA'].includes(t.tagName)) return;
		const a = keyAction(e);
		if (!a) return;
		const p = current;
		switch (a) {
			case 'keep':
				if (p) decide(p, 'keep');
				break;
			case 'reject':
				if (p) decide(p, 'reject');
				break;
			case 'move':
				void openPicker(p);
				break;
			case 'next':
				advance();
				break;
			case 'prev':
				cursor = step(cursor, -1, items.length);
				break;
			case 'undo':
				undo();
				break;
			case 'focus':
				focusMode = !focusMode;
				break;
			case 'close':
				if (!focusMode) return;
				focusMode = false;
				break;
		}
		e.preventDefault();
	}

	// keep the current card in view while stepping through the grid
	$effect(() => {
		const i = cursor;
		if (focusMode || !grid) return;
		grid.querySelector<HTMLElement>(`[data-index="${i}"]`)?.scrollIntoView({ block: 'nearest' });
	});

	onMount(() => {
		void loadPage(true);
		void loadStats();
	});
</script>

<svelte:head><title>Review live pictures · PencilPilot (dev)</title></svelte:head>
<svelte:window onkeydown={onKey} />

<div class="pp-page">
	<span class="pp-eyebrow"><a href="/dev">Dev tools</a> · ADR-0009</span>
	<h1>Review live pictures</h1>
	<!-- focus mode keeps the picture on screen: intro and per-topic stats step aside -->
	{#if !focusMode}
		<p class="lede">
			What the picture sorter let into the live pool. Keep what's right, move what's in the wrong
			topic, reject the rest. Your decisions win over later rule changes and measure the sorter (<code>
				ref-service recheck --labels
			</code>). Keys: <kbd>K</kbd> keep, <kbd>R</kbd>/<kbd>X</kbd>
			reject, <kbd>M</kbd> move, <kbd>←</kbd>/<kbd>J</kbd> and <kbd>→</kbd>/<kbd>L</kbd> previous
			and next, <kbd>Z</kbd> undo, <kbd>F</kbd> focus mode.
		</p>

		<section class="stats" aria-label="Sorter precision per topic">
			<p class="stats-total">
				<b>{statsTotal.reviewed}</b> of {statsTotal.pictures + statsTotal.rejected} reviewed{#if statsTotal.reviewed}
					&nbsp;· <b>{keptPercent(statsTotal)}%</b> kept unchanged{/if}
			</p>
			<ul>
				{#each visibleStats as s (`${s.category}/${s.topic}`)}
					<li>
						<button
							type="button"
							class="chip"
							class:on={filters.topic === s.topic && filters.category === s.category}
							onclick={() => {
								filters.category = s.category;
								setFilter('topic', s.topic);
							}}
						>
							<span class="chip-name">
								{catName(s.category)} › {topicLabel(s.category, s.topic)}
							</span>
							<span class="chip-num">
								{s.reviewed}/{s.pictures + s.rejected}
								{#if keptPercent(s) !== null}· {keptPercent(s)}% kept{/if}
							</span>
						</button>
					</li>
				{/each}
			</ul>
		</section>
	{:else}
		<p class="stats-total">
			<b>{statsTotal.reviewed}</b> of {statsTotal.pictures + statsTotal.rejected} reviewed{#if statsTotal.reviewed}
				&nbsp;· <b>{keptPercent(statsTotal)}%</b> kept unchanged{/if} · keys K R M J/L Z, Esc for the
			grid
		</p>
	{/if}

	<form class="filters" aria-label="Filters" onsubmit={(e) => e.preventDefault()}>
		<label>
			Category
			<select
				value={filters.category}
				onchange={(e) => setFilter('category', e.currentTarget.value)}
			>
				<option value="">All</option>
				{#each data.categories as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
			</select>
		</label>
		<label>
			Topic
			<select
				value={filters.topic}
				disabled={!filters.category}
				onchange={(e) => setFilter('topic', e.currentTarget.value)}
			>
				<option value="">All</option>
				{#each data.topics[filters.category] ?? [] as t (t.id)}
					<option value={t.id}>{t.name}</option>
				{/each}
			</select>
		</label>
		<label>
			Source
			<select value={filters.source} onchange={(e) => setFilter('source', e.currentTarget.value)}>
				<option value="">All</option>
				{#each SOURCES as s (s)}<option value={s}>{s}</option>{/each}
			</select>
		</label>
		<label>
			Tier
			<select value={filters.tier} onchange={(e) => setFilter('tier', e.currentTarget.value)}>
				<option value="">All</option>
				{#each TIERS as t (t.id)}<option value={String(t.id)}>{t.name}</option>{/each}
			</select>
		</label>
		<label>
			Status
			<select
				value={filters.status}
				onchange={(e) => setFilter('status', e.currentTarget.value as Filters['status'])}
			>
				<option value="unreviewed">Not reviewed</option>
				<option value="reviewed">Reviewed</option>
				<option value="all">All</option>
			</select>
		</label>
		<label>
			Sort
			<select
				value={filters.sort}
				onchange={(e) => setFilter('sort', e.currentTarget.value as Filters['sort'])}
			>
				<option value="newest">Newest</option>
				<option value="reported">Most reported</option>
				<option value="random">Random</option>
			</select>
		</label>
		<div class="filter-actions">
			<Button
				variant={focusMode ? 'dark' : 'soft'}
				aria-pressed={focusMode}
				aria-keyshortcuts="F"
				onclick={() => (focusMode = !focusMode)}
			>
				Focus mode
			</Button>
			<Button variant="outline" aria-keyshortcuts="Z" disabled={!undoCount} onclick={undo}>
				Undo
			</Button>
		</div>
	</form>

	<p class="status" role="status" aria-live="polite">
		{#if message}{message}{:else if loading}Loading…{:else if !apiError}{items.length} of {total}
			shown{/if}
	</p>
	{#if apiError}
		<p class="error" role="alert">{apiError}</p>
	{/if}

	{#if focusMode && current}
		{@const p = current}
		<Tile as="section" label="Focus review">
			<div class="focus">
				<div class="focus-img">
					<img src={p.imageUrl} alt={p.title || p.tags.join(', ')} />
				</div>
				<div class="focus-side">
					<p class="pp-eyebrow">{cursor + 1} of {total} · {reviewLabel(p)}</p>
					<h2 class="focus-topic">
						<span>{catName(p.category)}</span>
						{topicLabel(p.category, p.topic)}
					</h2>
					<p class="small">
						{tierName(p.tier)} · {p.kind || 'unknown kind'} · {p.sorted
							? 'sorted by the model'
							: 'filed by search only'}
					</p>
					{@render credit(p)}
					{@render reports(p)}
					<div class="focus-actions">
						<Button
							variant="dark"
							aria-keyshortcuts="K"
							disabled={p.rejected}
							onclick={() => decide(p, 'keep')}
						>
							Keep <kbd>K</kbd>
						</Button>
						<Button
							variant="primary"
							aria-keyshortcuts="R X"
							disabled={p.rejected}
							onclick={() => decide(p, 'reject')}
						>
							Reject <kbd>R</kbd>
						</Button>
						<Button
							variant="outline"
							aria-keyshortcuts="M"
							disabled={p.rejected}
							onclick={() => openPicker(p)}
						>
							Move… <kbd>M</kbd>
						</Button>
					</div>
					<div class="focus-actions">
						<Button
							variant="soft"
							aria-keyshortcuts="ArrowLeft J"
							disabled={cursor === 0}
							onclick={() => (cursor = step(cursor, -1, items.length))}
						>
							← Previous
						</Button>
						<Button
							variant="soft"
							aria-keyshortcuts="ArrowRight L"
							disabled={cursor >= items.length - 1}
							onclick={advance}
						>
							Next →
						</Button>
					</div>
				</div>
			</div>
		</Tile>
	{:else if focusMode && !loading}
		<p>Nothing to review with these filters.</p>
	{/if}

	{#if !focusMode}
		{#if !items.length && !loading && !apiError}
			<p>Nothing to review with these filters.</p>
		{/if}
		<ul class="grid" bind:this={grid} aria-label="Pictures">
			{#each items as p, i (p.id)}
				<li
					class="card"
					class:current={i === cursor}
					class:rejected={p.rejected}
					data-index={i}
					aria-current={i === cursor ? 'true' : undefined}
				>
					<button
						type="button"
						class="thumb"
						aria-label="Select picture {p.id}"
						onclick={() => (cursor = i)}
						ondblclick={() => ((cursor = i), (focusMode = true))}
					>
						<img src={p.imageUrl} alt={p.title || p.tags.join(', ')} loading="lazy" />
					</button>
					<div class="place">
						<b>{catName(p.category)} › {topicLabel(p.category, p.topic)}</b>
						<span class="badge" class:done={!!p.review}>{reviewLabel(p)}</span>
					</div>
					{@render credit(p)}
					{@render reports(p)}
					<div class="row">
						<button
							type="button"
							class="act keep"
							disabled={p.rejected}
							onclick={() => decide(p, 'keep')}
						>
							Keep
						</button>
						<button
							type="button"
							class="act reject"
							disabled={p.rejected}
							onclick={() => decide(p, 'reject')}
						>
							Reject
						</button>
					</div>
					<div class="row">
						<select aria-label="Move {p.id} to" bind:value={moveChoice[p.id]} disabled={p.rejected}>
							<option value="">Move to…</option>
							{#each p.moveTo as c (c)}
								<optgroup label={catName(c)}>
									{#each data.topics[c] ?? [] as t (t.id)}
										{#if !(c === p.category && t.id === p.topic)}
											<option value="{c}/{t.id}">{t.name}</option>
										{/if}
									{/each}
								</optgroup>
							{/each}
						</select>
						<button type="button" class="act" disabled={p.rejected} onclick={() => moveFromCard(p)}>
							Move
						</button>
					</div>
					<label class="small tier">
						Tier
						<select
							value={p.tier}
							disabled={p.rejected}
							onchange={(e) => decide(p, 'tier', { tier: Number(e.currentTarget.value) })}
						>
							{#each TIERS as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
						</select>
					</label>
				</li>
			{/each}
		</ul>
		{#if items.length < total}
			<Button variant="soft" disabled={loading} onclick={() => loadPage(false)}>Load more</Button>
		{/if}
	{/if}
</div>

<dialog bind:this={picker} aria-labelledby="picker-title" onclose={() => (pickerFor = null)}>
	{#if pickerFor}
		<h2 id="picker-title">Move to…</h2>
		<form
			method="dialog"
			onsubmit={(e) => {
				e.preventDefault();
				if (pickerOptions[0]) pick(pickerOptions[0]);
			}}
		>
			<label>
				Filter topics (Enter picks the first)
				<!-- svelte-ignore a11y_autofocus -->
				<input bind:value={pickerQuery} autofocus autocomplete="off" />
			</label>
		</form>
		<ul class="picker-list">
			{#each pickerOptions as o (`${o.category}/${o.topic}`)}
				<li>
					<button type="button" onclick={() => pick(o)}>
						<span class="small">{catName(o.category)} ›</span>
						{o.name}
					</button>
				</li>
			{:else}
				<li class="small">No matching topic this picture may go in.</li>
			{/each}
		</ul>
		<button type="button" class="act" onclick={() => picker?.close()}>Cancel (Esc)</button>
	{/if}
</dialog>

{#snippet credit(p: Picture)}
	<p class="small credit">
		{#if p.credit.pageUrl}<a href={p.credit.pageUrl} target="_blank" rel="noreferrer">
				{p.id}
			</a>{:else}{p.id}{/if}
		· {p.credit.author || 'unknown'} · {p.provider} ({p.source}) · {p.credit.license}
	</p>
{/snippet}

{#snippet reports(p: Picture)}
	{#if p.reports.count}
		<p class="small reported">
			Reported {p.reports.count}×{#if p.reports.reasons.length}: {p.reports.reasons.join('; ')}{/if}
			{#if p.blocked}(blocked; Keep lifts it){/if}
		</p>
	{/if}
{/snippet}

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 8px;
	}
	.lede {
		color: var(--pp-ink-2);
		max-width: 900px;
		line-height: 1.6;
	}
	kbd {
		font: 600 12px var(--pp-font-body);
		padding: 1px 6px;
		border-radius: 6px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
	}
	.stats {
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		padding: 16px;
		margin: 16px 0;
	}
	.stats-total {
		margin: 0 0 10px;
	}
	.stats ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		max-height: 160px;
		overflow-y: auto;
	}
	.chip {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		padding: 8px 12px;
		border-radius: 14px;
		border: 2px solid var(--pp-track);
		background: var(--pp-ground);
		font: 13px var(--pp-font-body);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.chip.on {
		border-color: var(--pp-ink);
	}
	.chip-name {
		font-weight: 600;
	}
	.chip-num {
		color: var(--pp-muted);
	}
	.filters {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
		align-items: flex-end;
		margin-bottom: 8px;
	}
	.filters label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
	}
	.filter-actions {
		display: flex;
		gap: 8px;
		margin-left: auto;
	}
	select,
	input {
		padding: 8px;
		border-radius: 10px;
		border: 2px solid var(--pp-track);
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: inherit;
		font-size: 14px;
		min-width: 0;
	}
	.status {
		min-height: 1.4em;
		color: var(--pp-ink-2);
	}
	.error {
		background: var(--pp-yellow);
		padding: 12px 16px;
		border-radius: 14px;
		font-weight: 600;
	}
	.grid {
		list-style: none;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
		gap: 16px;
	}
	.card {
		background: var(--pp-surface);
		border-radius: var(--pp-radius-card);
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		border: 3px solid transparent;
		min-width: 0;
	}
	.card.current {
		border-color: var(--pp-ink);
	}
	.card.rejected {
		opacity: 0.45;
	}
	.thumb {
		padding: 0;
		border: 0;
		background: var(--pp-ground);
		border-radius: 14px;
		cursor: pointer;
		aspect-ratio: 1;
		overflow: hidden;
	}
	.thumb img {
		width: 100%;
		height: 100%;
		object-fit: contain;
		display: block;
	}
	.place {
		display: flex;
		justify-content: space-between;
		gap: 8px;
		font-size: 14px;
	}
	.badge {
		font-size: 12px;
		padding: 2px 8px;
		border-radius: 999px;
		background: var(--pp-track);
		white-space: nowrap;
		align-self: flex-start;
	}
	.badge.done {
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
	}
	.small {
		font-size: 12px;
		color: var(--pp-muted);
		margin: 0;
		overflow-wrap: anywhere;
	}
	.reported {
		color: var(--pp-accent-ink);
		font-weight: 600;
	}
	.row {
		display: flex;
		gap: 6px;
	}
	.row select {
		flex: 1;
	}
	.tier {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.act {
		padding: 8px 14px;
		border: 2px solid transparent;
		border-radius: 10px;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
		background: var(--pp-ground);
		color: var(--pp-ink);
	}
	.act:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.row .keep,
	.row .reject {
		flex: 1;
	}
	.keep {
		background: var(--pp-ink);
		color: #fff;
	}
	.reject {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.focus {
		display: grid;
		grid-template-columns: minmax(0, 2fr) minmax(260px, 1fr);
		gap: 24px;
		align-items: start;
	}
	.focus-img {
		background: var(--pp-ground);
		border-radius: var(--pp-radius-card);
		display: flex;
		justify-content: center;
	}
	.focus-img img {
		max-width: 100%;
		max-height: calc(100vh - 380px);
		min-height: 240px;
		object-fit: contain;
		display: block;
	}
	.focus-side {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.focus-topic {
		font-size: clamp(32px, 4vw, 52px);
		line-height: 1;
	}
	.focus-topic span {
		display: block;
		font-size: 0.45em;
		color: var(--pp-muted);
		margin-bottom: 4px;
	}
	.focus-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.focus-actions kbd {
		background: rgb(255 255 255 / 0.25);
		color: inherit;
	}
	@media (max-width: 760px) {
		.focus {
			grid-template-columns: minmax(0, 1fr);
		}
		.filter-actions {
			margin-left: 0;
		}
	}
	dialog {
		border: 0;
		border-radius: var(--pp-radius-card);
		padding: 24px;
		width: min(520px, calc(100vw - 32px));
		max-height: 80vh;
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	dialog::backdrop {
		background: rgb(22 22 26 / 0.5);
	}
	dialog label {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-size: 13px;
		font-weight: 600;
		margin: 12px 0;
	}
	.picker-list {
		list-style: none;
		padding: 0;
		margin: 0 0 12px;
		max-height: 50vh;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.picker-list button {
		width: 100%;
		text-align: left;
		padding: 10px 12px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-ground);
		font: 15px var(--pp-font-body);
		color: var(--pp-ink);
		cursor: pointer;
	}
</style>
