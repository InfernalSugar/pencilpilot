<script lang="ts">
	/** Shown at start-up when unsaved work was left behind (the app closed unexpectedly). */
	import { onMount } from 'svelte';
	import { ago, projectName, type RecoveryItem } from './project';

	let {
		items,
		onrecover,
		ondiscard,
		onlater
	}: {
		items: RecoveryItem[];
		onrecover: (item: RecoveryItem) => void;
		ondiscard: (item: RecoveryItem) => void;
		/** close for now; the copies stay for next time */
		onlater: () => void;
	} = $props();

	let dialog = $state<HTMLDialogElement>();
	onMount(() => dialog?.showModal());
	$effect(() => {
		if (!items.length) dialog?.close();
	});
</script>

<dialog
	bind:this={dialog}
	class="recover"
	aria-labelledby="recover-title"
	oncancel={onlater}
	onclose={onlater}
>
	<h2 id="recover-title">Unsaved work was found</h2>
	<p>PencilPilot closed before {items.length === 1 ? 'this was' : 'these were'} saved.</p>
	<ul>
		{#each items as item (item.id)}
			<li>
				{#if item.thumbnail}
					<img src="data:image/png;base64,{item.thumbnail}" alt="" />
				{:else}
					<span class="blank" aria-hidden="true"></span>
				{/if}
				<div class="what">
					<b>{projectName(item.path)}</b>
					<span>{item.width} × {item.height} · {ago(item.savedAt)}</span>
				</div>
				<button type="button" class="go" onclick={() => onrecover(item)}>Recover</button>
				<button type="button" onclick={() => ondiscard(item)}>Discard</button>
			</li>
		{/each}
	</ul>
	<button type="button" class="later" onclick={() => dialog?.close()}>Decide later</button>
</dialog>

<style>
	.recover {
		width: min(560px, calc(100vw - 32px));
		padding: 24px;
		border: 0;
		border-radius: 22px;
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	.recover::backdrop {
		background: rgba(22, 22, 26, 0.45);
	}
	h2 {
		margin: 0 0 4px;
		font: 700 22px var(--pp-font-display);
	}
	p {
		margin: 0 0 16px;
		color: var(--pp-muted);
	}
	ul {
		display: grid;
		gap: 10px;
		margin: 0 0 16px;
		padding: 0;
		list-style: none;
	}
	li {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 10px;
		border-radius: 16px;
		background: var(--pp-ground);
	}
	img,
	.blank {
		width: 72px;
		height: 48px;
		object-fit: contain;
		border-radius: 8px;
		background: #fff;
	}
	.what {
		flex: 1;
		display: grid;
		gap: 2px;
		min-width: 0;
	}
	.what span {
		color: var(--pp-muted);
		font-size: 13px;
	}
	button {
		min-height: 40px;
		padding: 0 14px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.go {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.later {
		background: none;
		text-decoration: underline;
	}
</style>
