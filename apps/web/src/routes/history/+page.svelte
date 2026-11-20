<script lang="ts">
	import { onMount } from 'svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import {
		HISTORY_SIZE,
		deleteDrawing,
		listDrawings,
		pngToGray,
		pngToRgba,
		type SavedDrawing
	} from '#lib/history.js';
	import type { PsdGroup, PsdLayer } from '@pencilpilot/paint/psd';

	let drawings = $state<SavedDrawing[]>([]);
	let loaded = $state(false);
	let confirming = $state<string | null>(null);
	let busy = $state<string | null>(null);

	onMount(() => {
		void listDrawings().then((d) => {
			drawings = d;
			loaded = true;
		});
		// a drawing you just left may still be saving: look again shortly
		const again = setTimeout(async () => (drawings = await listDrawings()), 1000);
		return () => clearTimeout(again);
	});

	const when = (t: number) =>
		new Date(t).toLocaleString(undefined, {
			day: 'numeric',
			month: 'short',
			hour: '2-digit',
			minute: '2-digit'
		});

	const continueHref = (d: SavedDrawing) =>
		`/draw?history=${encodeURIComponent(d.id)}` +
		(d.category ? `&category=${encodeURIComponent(d.category)}` : '') +
		(d.mode === 'sketch' ? '&mode=sketch' : '');

	const slug = (d: SavedDrawing) =>
		`pencilpilot-${
			d.title
				.toLowerCase()
				.replace(/[^a-z0-9]+/g, '-')
				.replace(/^-|-$/g, '') || 'drawing'
		}-${new Date(d.savedAt).toISOString().slice(0, 10)}`;

	function download(blob: Blob, name: string) {
		const a = document.createElement('a');
		a.href = URL.createObjectURL(blob);
		a.download = name;
		a.click();
		setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
	}

	async function savePsd(d: SavedDrawing) {
		busy = d.id;
		try {
			const { width: w, height: h } = d;
			const { writePsd } = await import('@pencilpilot/paint/psd');
			const items: (PsdLayer | PsdGroup)[] = [];
			let open = null as { id: number; folder: PsdGroup } | null;
			for (const l of d.layers) {
				const layer: PsdLayer = {
					kind: 'layer',
					name: l.name,
					rgba: await pngToRgba(l.png, w, h),
					opacity: l.opacity,
					blend: l.blend,
					visible: l.visible,
					clip: l.clip,
					...(l.mask ? { mask: await pngToGray(l.mask, w, h) } : {})
				};
				const g = l.group ? d.groups.find((x) => x.id === l.group) : undefined;
				if (!g) {
					open = null;
					items.push(layer);
				} else if (open?.id === g.id) open.folder.children.push(layer);
				else {
					const folder: PsdGroup = {
						kind: 'group',
						name: g.name,
						opacity: g.opacity,
						visible: g.visible,
						children: [layer]
					};
					open = { id: g.id, folder };
					items.push(folder);
				}
			}
			const bytes = writePsd({
				width: w,
				height: h,
				composite: await pngToRgba(d.png, w, h),
				items
			});
			download(
				new Blob([bytes as BlobPart], { type: 'image/vnd.adobe.photoshop' }),
				`${slug(d)}.psd`
			);
		} finally {
			busy = null;
		}
	}

	async function remove(d: SavedDrawing) {
		await deleteDrawing(d.id);
		drawings = drawings.filter((x) => x.id !== d.id);
		confirming = null;
	}
</script>

<svelte:head><title>Your drawings · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">History</span>
	<h1>Your last {HISTORY_SIZE} drawings</h1>
	<p class="lead">
		Pick one up where you left off, or download it. They’re kept in this browser on this device; the
		oldest drops off when a new one comes in.
	</p>

	{#if loaded && !drawings.length}
		<div class="empty">
			<h2>Nothing here yet</h2>
			<p>Your drawings show up here as you draw, even ones you haven’t handed in.</p>
			<Button href="/draw">Start drawing</Button>
		</div>
	{:else}
		<ul class="grid" aria-label="Your drawings">
			{#each drawings as d (d.id)}
				<li class="card">
					<a class="thumb" href={continueHref(d)} aria-label="Keep drawing {d.title}">
						<img src={d.thumb} alt="" />
					</a>
					<div class="info">
						<b>{d.title}</b>
						<span class="meta">
							{when(d.savedAt)} ·
							{#if d.submitted && d.score !== undefined}
								<span class="score">{d.score.toFixed(1)}</span>
							{:else if d.submitted}
								handed in
							{:else}
								<span class="draft">draft</span>
							{/if}
							· {d.layers.length} layer{d.layers.length === 1 ? '' : 's'}
						</span>
					</div>
					<div class="actions">
						<Button href={continueHref(d)}>Keep drawing</Button>
						<button
							type="button"
							class="chip"
							onclick={() => download(d.png, `${slug(d)}.png`)}
							aria-label="Download {d.title} as a PNG picture"
						>
							PNG
						</button>
						<button
							type="button"
							class="chip"
							disabled={busy === d.id}
							onclick={() => savePsd(d)}
							aria-label="Download {d.title} as a Photoshop file"
						>
							{busy === d.id ? 'Saving…' : 'PSD'}
						</button>
						{#if confirming === d.id}
							<span class="warn">Delete for good?</span>
							<button type="button" class="chip danger" onclick={() => remove(d)}>Delete</button>
							<button type="button" class="chip" onclick={() => (confirming = null)}>Keep</button>
						{:else}
							<button
								type="button"
								class="chip"
								aria-label="Delete {d.title}"
								onclick={() => (confirming = d.id)}
							>
								Delete
							</button>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	h1 {
		margin: 4px 0 8px;
	}
	.lead {
		max-width: 62ch;
		margin: 0 0 24px;
		color: var(--pp-ink-2);
	}
	.empty {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 10px;
		padding: 24px;
		border-radius: var(--pp-radius-card);
		background: var(--pp-surface);
		max-width: 560px;
	}
	.empty h2,
	.empty p {
		margin: 0;
	}
	.grid {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
		gap: var(--pp-gap);
	}
	.card {
		display: flex;
		flex-direction: column;
		gap: 10px;
		padding: 12px;
		border-radius: var(--pp-radius-card);
		background: var(--pp-surface);
	}
	.thumb {
		display: block;
		border-radius: 14px;
		overflow: hidden;
		background: #fff;
		border: 1px solid var(--pp-track);
	}
	.thumb img {
		display: block;
		width: 100%;
		aspect-ratio: 4 / 3;
		object-fit: contain;
	}
	.info {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}
	.info b {
		font: 700 17px var(--pp-font-display);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.meta {
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.score {
		padding: 1px 7px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 700;
	}
	.draft {
		padding: 1px 7px;
		border-radius: 999px;
		background: var(--pp-lilac);
		color: var(--pp-lilac-ink);
		font-weight: 700;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.actions :global(.btn) {
		min-height: 38px;
		padding: 8px 14px;
		font-size: 14px;
	}
	.chip {
		min-height: 38px;
		padding: 0 12px;
		border: 1px solid var(--pp-track);
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.chip:disabled {
		cursor: progress;
		color: var(--pp-ink-2);
	}
	.danger {
		border-color: var(--pp-accent-strong);
		color: var(--pp-accent-ink);
	}
	.warn {
		font-size: 13px;
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
</style>
