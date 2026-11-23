<script lang="ts">
	import { untrack } from 'svelte';
	import { previewUrl, type BrushManifest } from './brushes';
	import BrushCreator from './BrushCreator.svelte';
	import MyBrushThumb from './MyBrushThumb.svelte';
	import PenFeel from './PenFeel.svelte';
	import { customBrushes, isCustom, onCustomBrushes, type CustomBrush } from './custom-brushes';
	import { BRUSH_FILES, importBrushFiles, importNote } from './my-brush-import';

	let {
		manifest,
		selected = $bindable(),
		askToDelete = true
	}: {
		manifest: BrushManifest;
		selected: string;
		/** ask before deleting one of "My brushes" */
		askToDelete?: boolean;
	} = $props();

	const FAV_KEY = 'pp.favouriteBrushes';
	const readFavs = (): string[] => {
		try {
			return JSON.parse(localStorage.getItem(FAV_KEY) ?? '[]');
		} catch {
			return [];
		}
	};
	const initialFavs = readFavs();
	let favourites = $state<string[]>(initialFavs);
	// Open on the favourites tab if there are any, else on the selected brush's category.
	let mine = $state<CustomBrush[]>(customBrushes());
	$effect(() => onCustomBrushes(() => (mine = [...customBrushes()])));
	/** the brush creator, open on a brush (or on a new one: null) */
	let creator = $state<{ start: string | null; n: number } | null>(null);
	let opened = 0;
	const create = (start: string | null) => (creator = { start, n: ++opened });
	let note = $state('');
	let busy = $state(false);
	let files = $state<HTMLInputElement>();

	/** Bring brushes from other apps into My brushes, and open the first in the creator. */
	async function importFiles(list: File[], thenEdit: boolean): Promise<string> {
		if (!list.length) return '';
		busy = true;
		note = '';
		try {
			const r = await importBrushFiles(list);
			note = importNote(r);
			const first = r.brushes[0]?.id;
			if (first) {
				selected = first;
				category = 'mine';
				query = '';
				if (thenEdit) create(first);
			}
			return note;
		} finally {
			busy = false;
		}
	}

	let category = $state(
		untrack(() =>
			isCustom(selected)
				? 'mine'
				: initialFavs.length
					? 'favourites'
					: (manifest.brushes.find((b) => b.id === selected)?.category ?? manifest.categories[0].id)
		)
	);
	let query = $state('');

	const tabs = $derived([
		{ id: 'favourites', name: 'Favourites' },
		{ id: 'mine', name: `My brushes${mine.length ? ` (${mine.length})` : ''}` },
		...manifest.categories
	]);
	const builtIn = (id: string) => manifest.brushes.find((b) => b.id === id);
	const selectedName = $derived(
		isCustom(selected)
			? (mine.find((b) => b.id === selected)?.name ?? 'Brush')
			: (builtIn(selected)?.name ?? 'Brush')
	);
	const shown = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (q)
			return manifest.brushes.filter(
				(b) => b.name.toLowerCase().includes(q) || b.author.toLowerCase().includes(q)
			);
		if (category === 'favourites') return manifest.brushes.filter((b) => favourites.includes(b.id));
		return manifest.brushes.filter((b) => b.category === category);
	});

	function toggleFav(id: string) {
		favourites = favourites.includes(id) ? favourites.filter((f) => f !== id) : [...favourites, id];
		try {
			localStorage.setItem(FAV_KEY, JSON.stringify(favourites));
		} catch {
			/* storage unavailable: favourites last for this visit only */
		}
	}
</script>

<div class="picker">
	<div class="make">
		<button type="button" class="action" onclick={() => create(null)}>
			<svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
				<path d="M12 5v14M5 12h14" />
			</svg>
			New brush
		</button>
		<button type="button" class="action" disabled={busy} onclick={() => files?.click()}>
			<svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
				<path d="M12 4v11M7 10l5 5 5-5M5 20h14" />
			</svg>
			{busy ? 'Reading…' : 'Import brushes…'}
		</button>
		<input
			bind:this={files}
			class="visually-hidden"
			type="file"
			multiple
			accept={BRUSH_FILES}
			tabindex="-1"
			aria-label="Brush files to import"
			onchange={(e) => {
				const list = [...(e.currentTarget.files ?? [])];
				e.currentTarget.value = '';
				void importFiles(list, false);
			}}
		/>
	</div>
	{#if note}<p class="note" role="status">{note}</p>{/if}
	<button type="button" class="adjust" onclick={() => create(selected)}>
		{isCustom(selected) ? `Edit “${selectedName}”` : `Adjust “${selectedName}” and save as mine`}
	</button>
	<label class="search">
		<span class="visually-hidden">Search brushes</span>
		<input
			type="search"
			placeholder="Search {manifest.brushes.length} brushes…"
			bind:value={query}
		/>
	</label>
	{#if !query}
		<div class="tabs" role="tablist" aria-label="Brush categories">
			{#each tabs as t (t.id)}
				<button
					type="button"
					role="tab"
					aria-selected={category === t.id}
					onclick={() => (category = t.id)}
				>
					{t.name}
				</button>
			{/each}
		</div>
	{/if}
	{#if category === 'mine' && !query}
		<ul class="grid" aria-label="My brushes">
			{#each mine as b (b.id)}
				<li>
					<button
						type="button"
						class="brush"
						aria-pressed={selected === b.id}
						title={b.source ? b.name : `${b.name} · based on ${builtIn(b.base)?.name ?? b.base}`}
						onclick={() => (selected = b.id)}
					>
						{#key b}<MyBrushThumb brush={b} />{/key}
						<span class="name">{b.name}</span>
					</button>
					<button
						type="button"
						class="corner"
						aria-label="Edit {b.name}"
						title="Edit"
						onclick={() => {
							selected = b.id;
							create(b.id);
						}}
					>
						<svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true">
							<path d="M4 20h4L19 9l-4-4L4 16Z M14 6l4 4" />
						</svg>
					</button>
				</li>
			{:else}
				<li class="empty">
					Make one with “New brush”, adjust any brush, or import brushes from another app.
				</li>
			{/each}
		</ul>
	{:else}
		<ul class="grid" aria-label="Brushes">
			{#each shown as b (b.id)}
				<li>
					<button
						type="button"
						class="brush"
						aria-pressed={selected === b.id}
						title="{b.name} · {b.author}"
						onclick={() => (selected = b.id)}
					>
						{#if b.preview}
							<img src={previewUrl(b.id)} alt="" loading="lazy" width="64" height="64" />
						{/if}
						<span class="name">{b.name}</span>
					</button>
					<button
						type="button"
						class="corner fav"
						aria-label="{favourites.includes(b.id)
							? 'Remove'
							: 'Add'} {b.name} {favourites.includes(b.id) ? 'from' : 'to'} favourites"
						aria-pressed={favourites.includes(b.id)}
						onclick={() => toggleFav(b.id)}
					>
						<svg width="14" height="14" viewBox="0 0 24 24" aria-hidden="true">
							<path
								d="M12 3l2.7 5.6 6.1.8-4.5 4.2 1.1 6.1L12 16.8l-5.4 2.9 1.1-6.1-4.5-4.2 6.1-.8Z"
							/>
						</svg>
					</button>
				</li>
			{:else}
				<li class="empty">
					{category === 'favourites' && !query
						? 'Tap the star on any brush to keep it here.'
						: 'No brushes match.'}
				</li>
			{/each}
		</ul>
	{/if}
	<PenFeel />
</div>

{#if creator}
	{#key creator.n}
		<BrushCreator
			{manifest}
			{askToDelete}
			start={creator.start}
			ondone={(id) => {
				creator = null;
				if (id) {
					selected = id;
					if (isCustom(id)) category = 'mine';
				}
			}}
			onimport={(list) => importFiles(list, true)}
		/>
	{/key}
{/if}

<style>
	.picker {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-height: 0;
	}
	.make {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
	}
	.action {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 6px;
		flex: 1 1 auto;
		min-height: 40px;
		padding: 0 10px;
		border: 0;
		border-radius: 12px;
		white-space: nowrap;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.action + .action {
		background: var(--pp-ground);
		color: var(--pp-ink);
	}
	.action:disabled {
		opacity: 0.6;
		cursor: default;
	}
	.action svg {
		fill: none;
		stroke: currentColor;
		stroke-width: 2.4;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.note {
		margin: -4px 0 0;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.adjust {
		padding: 10px 12px;
		border: 2px dashed var(--pp-track);
		border-radius: 14px;
		background: transparent;
		font: 600 13px var(--pp-font-body);
		color: var(--pp-ink);
		text-align: left;
		cursor: pointer;
	}
	.adjust:hover {
		border-color: var(--pp-ink);
	}
	.search input {
		width: 100%;
		padding: 12px 14px;
		border: 2px solid var(--pp-track);
		border-radius: 14px;
		font: 500 15px var(--pp-font-body);
		background: var(--pp-ground);
	}
	.search input:focus {
		border-color: var(--pp-ink);
		outline: none;
	}
	.tabs {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.tabs button {
		padding: 7px 12px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-ground);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.tabs button[aria-selected='true'] {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(88px, 1fr));
		gap: 8px;
		overflow-y: auto;
		max-height: 360px;
	}
	li {
		position: relative;
	}
	.brush {
		width: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding: 8px 6px;
		border: 2px solid transparent;
		border-radius: 16px;
		background: var(--pp-ground);
		cursor: pointer;
		font: 500 12px/1.2 var(--pp-font-body);
		color: var(--pp-ink);
	}
	.brush[aria-pressed='true'] {
		border-color: var(--pp-accent);
		background: var(--pp-surface);
	}
	/* dark interface: the previews keep their white paper */
	:global([data-theme='dark']) .brush img {
		mix-blend-mode: normal;
		background: #fff;
		border-radius: 10px;
	}
	.brush img {
		width: 56px;
		height: 56px;
		object-fit: contain;
		mix-blend-mode: multiply;
	}
	.name {
		text-align: center;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 100%;
	}
	.corner {
		position: absolute;
		top: 4px;
		right: 4px;
		width: 28px;
		height: 28px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 999px;
		background: transparent;
		cursor: pointer;
		fill: none;
		stroke: var(--pp-muted);
		stroke-width: 2;
		stroke-linejoin: round;
	}
	.corner:hover {
		stroke: var(--pp-ink);
	}
	.fav[aria-pressed='true'] {
		fill: var(--pp-yellow);
		stroke: var(--pp-ink);
	}
	.empty {
		grid-column: 1 / -1;
		padding: 20px;
		color: var(--pp-muted);
		font-size: 14px;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
