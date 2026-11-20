<script lang="ts">
	import { untrack } from 'svelte';
	import { previewUrl, type BrushManifest } from './brushes';
	import BrushEditor from './BrushEditor.svelte';
	import PenFeel from './PenFeel.svelte';
	import { customBrushes, isCustom, onCustomBrushes, type CustomBrush } from './custom-brushes';

	let {
		manifest,
		selected = $bindable()
	}: {
		manifest: BrushManifest;
		selected: string;
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
	let editing = $state(false);

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
	{#if editing}
		<BrushEditor
			{manifest}
			bind:selected
			onclose={() => {
				editing = false;
				if (isCustom(selected)) category = 'mine';
			}}
		/>
	{:else}
		<button type="button" class="adjust" onclick={() => (editing = true)}>
			{isCustom(selected)
				? `Adjust “${selectedName}”`
				: `Adjust “${selectedName}” and save as my brush`}
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
							title="{b.name} · based on {builtIn(b.base)?.name ?? b.base}"
							onclick={() => (selected = b.id)}
						>
							{#if builtIn(b.base)?.preview}
								<img src={previewUrl(b.base)} alt="" loading="lazy" width="64" height="64" />
							{/if}
							<span class="name">{b.name}</span>
						</button>
					</li>
				{:else}
					<li class="empty">Pick any brush and press “Adjust” above to make one that’s yours.</li>
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
							class="fav"
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
		<p class="credit">
			Brushes: <a
				href="https://github.com/mypaint/mypaint-brushes"
				target="_blank"
				rel="noreferrer"
			>
				MyPaint brush pack
			</a> (CC0) by its artists.
		</p>
	{/if}
</div>

<style>
	.picker {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-height: 0;
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
		color: #fff;
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
		background: #fff;
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
	.fav {
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
	.credit {
		margin: 0;
		font-size: 12px;
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
