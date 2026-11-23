<script lang="ts">
	/**
	 * The history panel (as in Photoshop): every step so far, newest at the top.
	 * Click one to go back to just after it; steps you went back past stay listed
	 * (greyed) until you draw something new, so you can come forward again.
	 */
	let {
		history,
		ongo
	}: {
		/** step names: `undo` oldest → newest (the last is the current state); `redo` next first */
		history: { undo: string[]; redo: string[] };
		/** move `steps` back (negative) or forward (positive) */
		ongo: (steps: number) => void;
	} = $props();

	/** All steps in order, each with how far away it is from now (0 = the current state). */
	const rows = $derived([
		...history.undo.map((label, i) => ({ label, at: i - (history.undo.length - 1), done: true })),
		...history.redo.map((label, i) => ({ label, at: i + 1, done: false }))
	]);
</script>

<section class="history" aria-label="History">
	<p class="hint">
		Click a step to go back to it. Draw something new and the greyed steps are gone.
	</p>
	<ol>
		{#each [...rows].reverse() as r (r.at)}
			<li>
				<button
					type="button"
					class:now={r.at === 0}
					class:undone={!r.done}
					aria-current={r.at === 0 ? 'step' : undefined}
					onclick={() => r.at !== 0 && ongo(r.at)}
				>
					{r.label}
				</button>
			</li>
		{/each}
		<li>
			<button
				type="button"
				class:now={history.undo.length === 0}
				aria-current={history.undo.length === 0 ? 'step' : undefined}
				onclick={() => history.undo.length && ongo(-history.undo.length)}
			>
				Start
			</button>
		</li>
	</ol>
</section>

<style>
	.history {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.hint {
		margin: 0;
		font-size: 13px;
		color: var(--pp-ink-2);
	}
	ol {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		max-height: 50vh;
		overflow-y: auto;
	}
	button {
		width: 100%;
		text-align: left;
		padding: 8px 12px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		font: 600 14px var(--pp-font-body);
		color: var(--pp-ink);
		cursor: pointer;
	}
	button:hover {
		background: var(--pp-surface-sunk);
	}
	button.now {
		background: var(--pp-ink);
		color: var(--pp-on-ink);
	}
	button.undone {
		color: var(--pp-muted);
		font-weight: 500;
	}
</style>
