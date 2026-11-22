<script lang="ts">
	/**
	 * Draw together (peer to peer): host a session and share its invite code, or join one
	 * with a friend's code. Shows who's here. Nothing connects until Host or Join.
	 */
	import { cleanInvite, memberColour, type ShareInfo } from './share';

	let {
		info,
		name = $bindable(),
		busy,
		error,
		onhost,
		onjoin,
		onleave,
		onclose
	}: {
		/** the session in progress */
		info: ShareInfo | null;
		/** this person's name, as others see it */
		name: string;
		busy: boolean;
		error: string;
		onhost: () => void;
		onjoin: (code: string) => void;
		onleave: () => void;
		onclose: () => void;
	} = $props();

	let el = $state<HTMLDialogElement>();
	$effect(() => {
		if (el && !el.open) el.showModal();
	});
	let code = $state('');
	let copied = $state(false);
	const cleaned = $derived(cleanInvite(code));

	async function copy() {
		if (!info) return;
		try {
			await navigator.clipboard.writeText(info.invite);
			copied = true;
			setTimeout(() => (copied = false), 1500);
		} catch {
			// the code stays selectable in the box
		}
	}
</script>

<dialog
	bind:this={el}
	class="share-dialog"
	aria-label="Draw together"
	oncancel={(e) => {
		e.preventDefault();
		onclose();
	}}
>
	<h2>Draw together</h2>
	{#if info}
		<p class="about">
			{info.host
				? 'You’re hosting. Send this code to the people you want to draw with:'
				: 'You’re in. Anyone with this code can join too:'}
		</p>
		<div class="code">
			<input
				readonly
				value={info.invite}
				aria-label="Invite code"
				onfocus={(e) => e.currentTarget.select()}
			/>
			<button type="button" onclick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</button>
		</div>
		<h3>Here now ({info.members.length})</h3>
		<ul class="members">
			{#each info.members as m (m.id)}
				<li>
					<i style="background: {memberColour(m.id)}"></i>
					{m.name}{m.id === info.me.id ? ' (you)' : ''}
				</li>
			{/each}
		</ul>
		<p class="note">
			The code is like a house key: anyone who has it can join. Drawings travel straight between
			your computers, encrypted.
		</p>
		<div class="actions">
			<button type="button" onclick={onleave} disabled={busy}>Leave</button>
			<button type="button" class="go" onclick={onclose}>Back to drawing</button>
		</div>
	{:else}
		<label class="name">
			<span>Your name</span>
			<input bind:value={name} maxlength="24" placeholder="What others see" />
		</label>
		<div class="ways">
			<section>
				<h3>Start a session</h3>
				<p>You host; friends join with your code.</p>
				<button type="button" class="go" disabled={busy || !name.trim()} onclick={onhost}>
					Host
				</button>
			</section>
			<section>
				<h3>Join a session</h3>
				<input bind:value={code} placeholder="Paste a pp1-… code" aria-label="Invite code" />
				<button
					type="button"
					class="go"
					disabled={busy || !name.trim() || !cleaned}
					onclick={() => cleaned && onjoin(cleaned)}
				>
					Join
				</button>
			</section>
		</div>
		<p class="note">
			Solo projects never connect to anything. Drawing together starts only when you host or join.
		</p>
		<div class="actions">
			<button type="button" onclick={onclose}>Close</button>
		</div>
	{/if}
	{#if busy}<p class="status" role="status">Connecting…</p>{/if}
	{#if error}<p class="error" role="alert">{error}</p>{/if}
</dialog>

<style>
	.share-dialog {
		width: min(520px, calc(100vw - 32px));
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.share-dialog::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 12px;
		font: 700 20px var(--pp-font-display);
	}
	h3 {
		margin: 12px 0 6px;
		font: 700 14px var(--pp-font-body);
	}
	input {
		width: 100%;
		box-sizing: border-box;
		padding: 8px 10px;
		border: 1px solid #d9d2c3;
		border-radius: 10px;
		background: #fff;
		font: inherit;
	}
	.name {
		display: grid;
		gap: 4px;
	}
	.ways {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 10px;
		margin-top: 12px;
	}
	.ways section {
		display: grid;
		gap: 8px;
		align-content: start;
		padding: 12px;
		border-radius: 12px;
		background: #f1ecdf;
	}
	.ways h3,
	.ways p {
		margin: 0;
	}
	.code {
		display: flex;
		gap: 8px;
	}
	.code input {
		font-family: ui-monospace, monospace;
		font-size: 12px;
	}
	.members {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 4px;
	}
	.members i {
		display: inline-block;
		width: 10px;
		height: 10px;
		margin-right: 6px;
		border-radius: 50%;
	}
	.about,
	.note {
		margin: 10px 0 0;
	}
	.note {
		color: #6b6559;
		font-size: 13px;
	}
	.status {
		margin: 10px 0 0;
	}
	.error {
		margin: 10px 0 0;
		color: #b3261e;
	}
	.actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
		margin-top: 16px;
	}
	button {
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
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	@media (max-width: 480px) {
		.ways {
			grid-template-columns: 1fr;
		}
	}
</style>
