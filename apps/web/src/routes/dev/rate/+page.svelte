<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	/** samples skipped this visit go to the back of the queue */
	let skipped = $state<string[]>([]);
	const queue = $derived([
		...data.todo.filter((s) => !skipped.includes(s.id)),
		...data.todo.filter((s) => skipped.includes(s.id))
	]);
	const s = $derived(queue[0]);
	let view = $state<'side' | 'overlay'>('side');
	let mix = $state(50);
	let confirmDelete = $state(false);
	let busy = $state(false);
	let message = $state('');
	let rateForm = $state<HTMLFormElement>();
	let rateValue = $state<HTMLInputElement>();

	function rate(n: number) {
		if (!s || busy || !rateForm || !rateValue) return;
		rateValue.value = String(n);
		rateForm.requestSubmit();
	}

	function skip() {
		if (s) skipped = [...skipped.filter((x) => x !== s.id), s.id];
		confirmDelete = false;
	}

	function onkey(e: KeyboardEvent) {
		if (e.ctrlKey || e.metaKey || e.altKey) return;
		const t = e.target as HTMLElement;
		if (t.closest('input, textarea, select')) return;
		if (/^[0-9]$/.test(e.key)) {
			e.preventDefault();
			rate(e.key === '0' ? 10 : Number(e.key));
		} else if (e.key === 's' || e.key === 'ArrowRight') skip();
		else if (e.key === 'o') view = view === 'side' ? 'overlay' : 'side';
	}

	const submitted = () => {
		busy = true;
		message = '';
		return async ({ result }: { result: { type: string } }) => {
			busy = false;
			confirmDelete = false;
			if (result.type === 'failure') message = 'That didn’t save. Try again.';
			await invalidateAll();
		};
	};
	const done = $derived(Object.values(data.rated).reduce((a, b) => a + b, 0));
</script>

<svelte:window onkeydown={onkey} />
<svelte:head><title>Rate drawings · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Dev only · teach the grader</span>
	<h1>Rate drawings</h1>
	<p class="lede">
		How good is this copy of the reference, the way an art teacher would mark it? 1 = nothing like
		it, 10 = spot on. The computer’s score is hidden on purpose, so it can’t sway you. Every drawing
		you hand in on the dev site lands here.
	</p>

	<section class="progress" aria-label="Ratings so far">
		<b>{done} rated</b>
		{#each Object.entries(data.rated) as [c, n] (c)}
			<span class="cat">
				{c}
				<span class="bar" aria-hidden="true">
					<span style="width: {Math.min(100, (n / data.target) * 100)}%"></span>
				</span>
				{n}/{data.target}
			</span>
		{/each}
		<span class="left">{queue.length} waiting</span>
	</section>

	{#if !s}
		<div class="empty">
			<h2>Nothing to rate right now</h2>
			<p>
				Draw and hand in on the dev site: every graded drawing is saved here automatically. Rough
				ones help as much as good ones, so mix in quick and careless attempts too.
			</p>
			{#if done >= 12}
				<p>
					Enough for a first fit: run <code>cargo run --release -p calibrate -- fit</code>.
				</p>
			{/if}
		</div>
	{:else}
		<div class="toolbar">
			<span class="meta">{s.category}{s.mode === 'sketch' ? ' · sketch' : ''}</span>
			<div class="seg" role="radiogroup" aria-label="View">
				<button
					type="button"
					role="radio"
					aria-checked={view === 'side'}
					onclick={() => (view = 'side')}
				>
					Side by side
				</button>
				<button
					type="button"
					role="radio"
					aria-checked={view === 'overlay'}
					onclick={() => (view = 'overlay')}
				>
					Overlay (O)
				</button>
			</div>
			{#if view === 'overlay'}
				<label class="mix">
					Drawing <input type="range" min="0" max="100" bind:value={mix} aria-label="Overlay mix" /> Reference
				</label>
			{/if}
		</div>

		{#key s.id}
			{#if view === 'side'}
				<div class="pair">
					<figure>
						<img src={s.refSrc} alt="Reference" />
						<figcaption>Reference</figcaption>
					</figure>
					<figure>
						<img src={s.drawingSrc} alt="The drawing to rate" />
						<figcaption>Drawing</figcaption>
					</figure>
				</div>
			{:else}
				<div class="overlay">
					<img src={s.drawingSrc} alt="The drawing to rate" />
					<img class="top" src={s.refSrc} alt="Reference" style="opacity: {mix / 100}" />
				</div>
			{/if}
		{/key}

		<form
			method="POST"
			action="?/rate"
			use:enhance={submitted}
			bind:this={rateForm}
			class="scale"
			aria-label="Your rating"
		>
			<input type="hidden" name="sample" value={s.id} />
			<input type="hidden" name="human" bind:this={rateValue} />
			{#each [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as n (n)}
				<button
					type="button"
					class="n"
					disabled={busy}
					aria-label="Rate {n} out of 10"
					onclick={() => rate(n)}
				>
					{n}
				</button>
			{/each}
		</form>
		<div class="row">
			<button type="button" class="ghost" onclick={skip}>Skip for now (S)</button>
			<form method="POST" action="?/delete" use:enhance={submitted}>
				<input type="hidden" name="sample" value={s.id} />
				{#if confirmDelete}
					<span class="warn">Delete this sample for good?</span>
					<button class="ghost danger" disabled={busy}>Delete</button>
					<button type="button" class="ghost" onclick={() => (confirmDelete = false)}>Keep</button>
				{:else}
					<button type="button" class="ghost" onclick={() => (confirmDelete = true)}>
						Not a real attempt? Delete
					</button>
				{/if}
			</form>
			<span class="keys">Keys: 1–9, 0 = 10 · S skip · O overlay</span>
		</div>
		{#if message}<p class="warn" role="alert">{message}</p>{/if}
	{/if}
</div>

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
	.progress {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 18px;
		margin: 12px 0 18px;
		font-size: 14px;
	}
	.cat {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--pp-ink-2);
	}
	.bar {
		display: inline-block;
		width: 70px;
		height: 8px;
		border-radius: 999px;
		background: var(--pp-track);
		overflow: hidden;
	}
	.bar span {
		display: block;
		height: 100%;
		background: var(--pp-accent);
	}
	.left {
		margin-left: auto;
		color: var(--pp-muted);
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px;
		margin-bottom: 12px;
	}
	.meta {
		font-weight: 700;
		text-transform: capitalize;
	}
	.seg {
		display: flex;
		gap: 2px;
		padding: 3px;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
	}
	.seg button {
		padding: 6px 12px;
		border: 0;
		border-radius: 9px;
		background: transparent;
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.seg button[aria-checked='true'] {
		background: var(--pp-surface);
	}
	.mix {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 16px;
	}
	@media (max-width: 700px) {
		.pair {
			grid-template-columns: 1fr;
		}
	}
	figure {
		margin: 0;
	}
	img {
		display: block;
		width: 100%;
		max-height: 62vh;
		aspect-ratio: 4 / 3;
		object-fit: contain;
		background: #fff;
		border-radius: 20px;
	}
	.overlay {
		position: relative;
		max-width: 900px;
	}
	.overlay .top {
		position: absolute;
		inset: 0;
		height: 100%;
		background: transparent;
	}
	figcaption {
		font-size: 13px;
		color: var(--pp-muted);
	}
	.scale {
		display: flex;
		gap: 8px;
		margin-top: 16px;
		flex-wrap: wrap;
	}
	.n {
		width: 56px;
		height: 56px;
		border: 0;
		border-radius: 16px;
		background: var(--pp-surface);
		font: 800 20px var(--pp-font-display);
		cursor: pointer;
	}
	.n:hover,
	.n:focus-visible {
		background: var(--pp-yellow);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		margin-top: 12px;
	}
	.row form {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.ghost {
		padding: 8px 12px;
		border: 1px solid var(--pp-track);
		border-radius: 12px;
		background: var(--pp-surface);
		font: 600 13px var(--pp-font-body);
		cursor: pointer;
	}
	.danger {
		border-color: var(--pp-accent-strong);
		color: var(--pp-accent-ink);
	}
	.warn {
		color: var(--pp-accent-ink);
		font-weight: 600;
		font-size: 13px;
	}
	.keys {
		margin-left: auto;
		font-size: 12px;
		color: var(--pp-muted);
	}
	.empty {
		padding: 24px;
		border-radius: 20px;
		background: var(--pp-surface);
		max-width: 700px;
	}
</style>
