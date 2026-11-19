<script lang="ts">
	import { goto } from '$app/navigation';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { OWN_STYLES, keepOwnPicture, type OwnStyle } from '#lib/own-picture.js';

	let file = $state<File | null>(null);
	let preview = $state('');
	let style = $state<OwnStyle>('real-life');
	let error = $state('');
	let busy = $state(false);
	let dragOver = $state(false);
	let input: HTMLInputElement;

	function choose(f: File | undefined | null) {
		error = '';
		if (!f) return;
		if (!f.type.startsWith('image/')) {
			error = 'That file isn’t a picture.';
			return;
		}
		if (f.size > 30 * 1024 * 1024) {
			error = 'That picture is too big (30 MB at most).';
			return;
		}
		file = f;
		if (preview) URL.revokeObjectURL(preview);
		preview = URL.createObjectURL(f);
	}

	async function start() {
		if (!file) return;
		busy = true;
		error = '';
		try {
			await keepOwnPicture(file, style);
			await goto('/draw?own=1');
		} catch (e) {
			error =
				e instanceof Error && e.message.includes('room')
					? e.message
					: 'That picture couldn’t be opened. Try another one.';
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Your own picture · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Practice</span>
	<h1>Practise with your own picture</h1>
	<p class="lead">
		Found something you want to draw: a photo you took, a screenshot, a picture from the web? Use it
		as your reference and get the same score and tips as with ours.
	</p>

	<div class="grid">
		<Tile as="section" label="Your picture">
			<h2>1. Your picture</h2>
			<button
				type="button"
				class="drop"
				class:over={dragOver}
				class:has={!!preview}
				onclick={() => input.click()}
				ondragover={(e) => {
					e.preventDefault();
					dragOver = true;
				}}
				ondragleave={() => (dragOver = false)}
				ondrop={(e) => {
					e.preventDefault();
					dragOver = false;
					choose(e.dataTransfer?.files?.[0]);
				}}
			>
				{#if preview}
					<img src={preview} alt="Preview of {file?.name ?? 'the chosen file'}" />
				{:else}
					<span class="big">Choose a picture</span>
					<span>or drop it here (JPEG, PNG, WebP…)</span>
				{/if}
			</button>
			<input
				bind:this={input}
				class="visually-hidden"
				type="file"
				accept="image/*"
				aria-label="Picture from your device"
				onchange={(e) => choose(e.currentTarget.files?.[0])}
			/>
			{#if file}
				<p class="small">
					{file.name} ·
					<button type="button" class="link" onclick={() => input.click()}>Choose another</button>
				</p>
			{/if}
			<p class="small private">
				Your picture stays on this device. It isn’t uploaded, saved on our server or shared.
			</p>
		</Tile>

		<Tile as="section" label="What kind of picture">
			<h2>2. What kind of picture is it?</h2>
			<p class="small">This sets what the score pays most attention to.</p>
			<div class="styles" role="radiogroup" aria-label="What kind of picture">
				{#each OWN_STYLES as s (s.id)}
					<label class="style" class:on={style === s.id}>
						<input type="radio" name="style" value={s.id} bind:group={style} />
						<b>{s.label}</b>
						<span>{s.blurb}</span>
					</label>
				{/each}
			</div>
			{#if error}<p class="error" role="alert">{error}</p>{/if}
			<Button onclick={start} disabled={!file || busy}>
				{busy ? 'Getting it ready…' : 'Start drawing →'}
			</Button>
			<p class="small">
				Drawings of your own pictures are scored with tips, but don’t count toward your category
				progress.
			</p>
		</Tile>
	</div>
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
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
		gap: var(--pp-gap);
		align-items: start;
	}
	h2 {
		font-size: 20px;
	}
	.drop {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 6px;
		min-height: 240px;
		padding: 16px;
		border: 2px dashed var(--pp-track);
		border-radius: 18px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink-2);
		font: 500 14px var(--pp-font-body);
		cursor: pointer;
	}
	.drop.over {
		border-color: var(--pp-accent);
	}
	.drop.has {
		padding: 8px;
		border-style: solid;
	}
	.drop img {
		max-width: 100%;
		max-height: 360px;
		border-radius: 12px;
		object-fit: contain;
	}
	.big {
		font: 700 18px var(--pp-font-display);
		color: var(--pp-ink);
	}
	.small {
		margin: 0;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.private {
		color: var(--pp-lilac-ink);
		font-weight: 600;
	}
	.link {
		padding: 0;
		border: 0;
		background: none;
		color: var(--pp-accent-ink);
		font: inherit;
		font-weight: 600;
		text-decoration: underline;
		cursor: pointer;
	}
	.styles {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
		gap: 8px;
	}
	.style {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 12px;
		border: 2px solid var(--pp-track);
		border-radius: 14px;
		cursor: pointer;
	}
	.style.on {
		border-color: var(--pp-ink);
		background: var(--pp-surface-sunk);
	}
	.style input {
		position: absolute;
		opacity: 0;
		pointer-events: none;
	}
	.style:has(input:focus-visible) {
		outline: 3px solid var(--pp-accent);
		outline-offset: 2px;
	}
	.style span {
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	.error {
		margin: 0;
		color: var(--pp-accent-ink);
		font-weight: 600;
	}
	.visually-hidden {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
	}
</style>
