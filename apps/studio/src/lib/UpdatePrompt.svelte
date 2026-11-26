<script lang="ts">
	/**
	 * A new version, offered in the corner (not in the way of drawing): what's new, then
	 * Update, Later or Skip this version. Updating downloads it here, asks "Save before
	 * updating?" if a project has changes, then installs and restarts.
	 */
	import { install, later, progressText, skip, updates } from './updates.svelte';

	const now = $derived(updates.now);
	let more = $state(false);
</script>

{#if now.stage !== 'none' && !updates.hidden}
	<section class="update" role="status" aria-label="A new version of PencilPilot">
		{#if now.stage === 'installing'}
			<h2>Installing PencilPilot {now.version}…</h2>
			<p>The app restarts by itself when it's done.</p>
		{:else}
			<h2>PencilPilot {now.version} is out</h2>
			{#if now.stage === 'failed'}
				<p class="bad">The update didn't work: {now.message}</p>
			{:else if now.stage === 'downloading'}
				<p>Downloading… {progressText(now.got, now.total)}</p>
				<progress max={now.total ?? undefined} value={now.total ? now.got : undefined}></progress>
			{:else if now.stage === 'ready'}
				<p>Downloaded. Restart to finish updating.</p>
			{/if}
			{#if now.notes && now.stage !== 'downloading'}
				<button type="button" class="link" onclick={() => (more = !more)} aria-expanded={more}>
					{more ? "Hide what's new" : "What's new"}
				</button>
				{#if more}<p class="notes">{now.notes}</p>{/if}
			{/if}
			<div class="actions">
				{#if now.stage === 'offered' || now.stage === 'failed'}
					<button type="button" onclick={skip}>Skip this version</button>
				{/if}
				<button type="button" onclick={later}>Later</button>
				<button
					type="button"
					class="go"
					disabled={now.stage === 'downloading'}
					onclick={() => void install()}
				>
					{now.stage === 'ready' ? 'Restart' : now.stage === 'failed' ? 'Try again' : 'Update'}
				</button>
			</div>
		{/if}
	</section>
{/if}

<style>
	.update {
		position: fixed;
		right: 16px;
		bottom: 16px;
		z-index: 1000;
		width: min(340px, calc(100vw - 32px));
		padding: 16px;
		border-radius: 16px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 16px 44px rgba(0, 0, 0, 0.28);
	}
	h2 {
		margin: 0 0 6px;
		font: 700 17px var(--pp-font-display);
	}
	p {
		margin: 0 0 6px;
	}
	.bad {
		color: var(--pp-accent-ink);
	}
	.notes {
		max-height: 180px;
		overflow: auto;
		white-space: pre-wrap;
		color: var(--pp-muted);
		-webkit-user-select: text;
		user-select: text;
	}
	progress {
		width: 100%;
		accent-color: var(--pp-accent);
	}
	.link {
		padding: 0;
		border: 0;
		background: none;
		color: var(--pp-accent-ink);
		font: inherit;
		cursor: pointer;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 12px;
	}
	.actions button {
		padding: 7px 12px;
		border: 0;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
		color: inherit;
		font: inherit;
		cursor: pointer;
	}
	.actions button.go {
		background: var(--pp-ink, #16161a);
		color: var(--pp-on-ink);
	}
	.actions button:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
