<script lang="ts">
	/**
	 * "Save before quitting?" (or before opening something else): Yes saves first, No
	 * goes ahead without saving, Cancel stays.
	 */
	let {
		title,
		name,
		onanswer
	}: {
		/** e.g. "Save before quitting?" */
		title: string;
		/** the project's name */
		name: string;
		onanswer: (choice: 'yes' | 'no' | 'cancel') => void;
	} = $props();

	let el = $state<HTMLDialogElement>();
	let yes = $state<HTMLButtonElement>();
	$effect(() => {
		if (el && !el.open) {
			el.showModal();
			yes?.focus();
		}
	});
</script>

<dialog
	bind:this={el}
	class="save-changes"
	aria-label={title}
	oncancel={(e) => {
		e.preventDefault();
		onanswer('cancel');
	}}
>
	<h2>{title}</h2>
	<p>“{name}” has changes that aren’t saved.</p>
	<div class="actions">
		<button type="button" onclick={() => onanswer('cancel')}>Cancel</button>
		<button type="button" onclick={() => onanswer('no')}>No</button>
		<button type="button" class="go" bind:this={yes} onclick={() => onanswer('yes')}>Yes</button>
	</div>
</dialog>

<style>
	.save-changes {
		width: min(420px, calc(100vw - 32px));
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.save-changes::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 8px;
		font: 700 20px var(--pp-font-display);
	}
	p {
		margin: 0;
	}
	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 18px;
	}
	button {
		min-width: 80px;
		padding: 8px 14px;
		border: 0;
		border-radius: 10px;
		background: #f1ecdf;
		color: inherit;
		font: inherit;
		cursor: pointer;
	}
	button.go {
		background: var(--pp-ink, #16161a);
		color: #fff;
	}
</style>
