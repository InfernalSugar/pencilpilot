<script lang="ts">
	/**
	 * Draw together (peer to peer): host a session and share its invite code, or join one
	 * with a friend's code. Shows who's here. Nothing connects until Host or Join, and both
	 * need a PencilPilot account (names come from it).
	 */
	import { onMount } from 'svelte';
	import { account, endAccount, loadAccount, startSignIn, stopSignIn } from './account.svelte';
	import {
		cleanInvite,
		inviteQr,
		memberColour,
		people,
		shareChoices,
		type ShareInfo
	} from './share';

	interface LayerRow {
		lid: string;
		name: string;
		/** a group of layers, not a layer */
		group?: boolean;
		owner: string;
		mine: boolean;
		canDraw: boolean;
		asked: boolean;
		writers: { id: string; name: string }[];
		/** `text`: what they ask, in words; `own`: to have it handed over; `draw`: to draw */
		asks: { id: string; name: string; text?: string; own?: boolean; draw?: boolean }[];
	}

	let {
		info,
		canRejoin = false,
		onrejoin = () => {},
		layers = [],
		synced = true,
		onask = () => {},
		ongrant = () => {},
		ondeny = () => {},
		onrevoke = () => {},
		busy,
		error,
		onhost,
		onjoin,
		onleave,
		onclose
	}: {
		/** the session in progress */
		info: ShareInfo | null;
		/** this project was shared: it can be rejoined */
		canRejoin?: boolean;
		onrejoin?: () => void;
		/** the shared layers, bottom → top */
		layers?: LayerRow[];
		/** the project has arrived (joining) */
		synced?: boolean;
		onask?: (lid: string) => void;
		ongrant?: (lid: string, to: string, handover: boolean) => void;
		ondeny?: (lid: string, to: string) => void;
		onrevoke?: (lid: string, to: string) => void;
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
	/** the invite as a QR code (an image), to scan with a phone or another screen */
	let qr = $state('');
	$effect(() => {
		const invite = info?.invite;
		qr = '';
		if (invite)
			inviteQr(invite)
				.then((svg) => (qr = `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`))
				.catch(() => {});
	});
	let copied = $state(false);
	const cleaned = $derived(cleanInvite(code));
	const can = $derived(shareChoices(account.me, busy));

	onMount(() => void loadAccount());
	// a refusal may mean the app was unlinked on the website: show it signed out
	$effect(() => {
		if (error) void loadAccount();
	});

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
		{#if qr}
			<img class="qr" src={qr} alt="The invite code as a QR code" width="140" height="140" />
		{/if}
		<h3>Here now ({people(info.members).length})</h3>
		<ul class="members">
			{#each people(info.members) as m (m.id)}
				<li>
					<i style="background: {memberColour(m.id)}"></i>
					{m.name}{m.id === info.me.id ? ' (you)' : ''}
				</li>
			{/each}
		</ul>
		{#if info.cloud && info.cloud !== 'off'}
			<p class="status" data-testid="cloud-status">
				{info.cloud === 'unreachable'
					? 'The cloud can’t be reached right now: this session isn’t kept there.'
					: info.cloud === 'caught_up'
						? 'No one else was online: you caught up from the cloud. It keeps what you draw.'
						: 'The cloud keeps a copy, so anyone in it can carry on when you’re offline.'}
			</p>
		{/if}
		{#if !synced}
			<p class="status" role="status">Getting the drawing from the others…</p>
		{:else if layers.length}
			<h3>Layers</h3>
			<ul class="layers">
				{#each [...layers].reverse() as l (l.lid)}
					<li>
						<div class="lrow">
							<span class="lname">{l.name}</span>
							<span class="owner">{l.mine ? 'yours' : l.owner}</span>
							{#if l.canDraw && !l.mine}
								<span class="tag">{l.group ? 'you can change it' : 'you can draw'}</span>
							{:else if !l.canDraw && !l.group}
								<button type="button" class="small" disabled={l.asked} onclick={() => onask(l.lid)}>
									{l.asked ? 'Asked' : 'Ask to draw here'}
								</button>
							{/if}
						</div>
						{#each l.asks as a (a.id)}
							<div class="ask">
								<span><b>{a.name}</b> {a.text ?? 'asks to draw here'}</span>
								{#if !a.own}
									<button
										type="button"
										class="small go"
										onclick={() => ongrant(l.lid, a.id, false)}
									>
										{a.draw === false ? 'Allow' : 'Let them draw'}
									</button>
								{/if}
								{#if a.own || a.draw !== false}
									<button
										type="button"
										class="small {a.own ? 'go' : ''}"
										onclick={() => ongrant(l.lid, a.id, true)}
									>
										Hand it over
									</button>
								{/if}
								<button type="button" class="small" onclick={() => ondeny(l.lid, a.id)}>No</button>
							</div>
						{/each}
						{#if l.mine && l.writers.length}
							<div class="ask">
								<span>
									{l.group ? 'Also changing it' : 'Also drawing'}: {l.writers
										.map((w) => w.name)
										.join(', ')}
								</span>
								{#each l.writers as w (w.id)}
									<button type="button" class="small" onclick={() => onrevoke(l.lid, w.id)}>
										Take back from {w.name}
									</button>
								{/each}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
		<p class="note">
			The code is like a house key: anyone who has it can join. Drawings travel straight between
			your computers, encrypted. Everyone keeps a full copy: save it any time.
		</p>
		<div class="actions">
			<button type="button" onclick={onleave} disabled={busy}>Leave</button>
			<button type="button" class="go" onclick={onclose}>Back to drawing</button>
		</div>
	{:else}
		<section class="account" aria-label="Your PencilPilot account">
			{#if account.me}
				<p>Signed in as <b>{account.me.name}</b></p>
				<button type="button" class="small" disabled={busy} onclick={() => void endAccount()}>
					Sign out
				</button>
			{:else if account.signingIn}
				<p>Finish signing in in your browser…</p>
				<button type="button" class="small" onclick={() => void stopSignIn()}>Cancel</button>
			{:else}
				<p>
					Drawing together needs a PencilPilot account, so everyone knows who they’re drawing with.
					Solo drawing never needs one.
				</p>
				<button
					type="button"
					class="go"
					disabled={!account.loaded || !can.signIn}
					onclick={() => void startSignIn()}
				>
					Sign in with PencilPilot
				</button>
			{/if}
			{#if account.error}<p class="error" role="alert">{account.error}</p>{/if}
		</section>
		{#if canRejoin}
			<section class="rejoin">
				<h3>This project is shared</h3>
				<p>
					Rejoin it: you’ll join whoever of its members is online, or, if no one is, catch up from
					the cloud and open it for them. What you drew on your own layers meanwhile is kept; what
					you drew on others’ is asked for.
				</p>
				<button type="button" class="go" disabled={!can.rejoin} onclick={onrejoin}>Rejoin</button>
			</section>
		{/if}
		<div class="ways">
			<section>
				<h3>Start a session</h3>
				<p>You host; friends join with your code.</p>
				<button type="button" class="go" disabled={!can.host} onclick={onhost}>Host</button>
			</section>
			<section>
				<h3>Join a session</h3>
				<input bind:value={code} placeholder="Paste a pp1-… code" aria-label="Invite code" />
				<button
					type="button"
					class="go"
					disabled={!can.join || !cleaned}
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
		border: 1px solid var(--pp-surface-sunk);
		border-radius: 10px;
		background: var(--pp-surface);
		font: inherit;
	}
	.account {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px 12px;
		padding: 12px;
		border-radius: 12px;
		background: #f1ecdf;
	}
	.account p {
		flex: 1 1 200px;
		margin: 0;
	}
	.account .error {
		flex-basis: 100%;
	}
	.rejoin {
		display: grid;
		gap: 8px;
		margin-top: 12px;
		padding: 12px;
		border-radius: 12px;
		background: var(--pp-good-bg);
	}
	.rejoin h3,
	.rejoin p {
		margin: 0;
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
		background: var(--pp-surface-sunk);
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
	.qr {
		display: block;
		margin: 10px auto 0;
		border-radius: 8px;
		background: var(--pp-surface);
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
	.layers {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 6px;
		max-height: 240px;
		overflow: auto;
	}
	.layers li {
		padding: 6px 8px;
		border-radius: 10px;
		background: var(--pp-surface-sunk);
	}
	.lrow,
	.ask {
		display: flex;
		align-items: center;
		gap: 8px;
		flex-wrap: wrap;
	}
	.ask {
		margin-top: 6px;
		font-size: 13px;
	}
	.lname {
		font-weight: 600;
	}
	.owner {
		color: var(--pp-muted);
		font-size: 13px;
		flex: 1;
	}
	.tag {
		font-size: 12px;
		color: #2a9d8f;
	}
	button.small {
		padding: 4px 10px;
		font-size: 13px;
		background: var(--pp-surface);
	}
	button.small.go {
		background: var(--pp-ink, #16161a);
		color: var(--pp-on-ink);
	}
	.about,
	.note {
		margin: 10px 0 0;
	}
	.note {
		color: var(--pp-muted);
		font-size: 13px;
	}
	.status {
		margin: 10px 0 0;
	}
	.error {
		margin: 10px 0 0;
		color: var(--pp-error);
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
		background: var(--pp-surface-sunk);
		color: inherit;
		font: inherit;
		cursor: pointer;
	}
	button.go {
		background: var(--pp-ink, #16161a);
		color: var(--pp-on-ink);
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
