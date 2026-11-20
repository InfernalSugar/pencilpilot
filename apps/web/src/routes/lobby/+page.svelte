<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';

	let code = $state('');
	let busy = $state(false);
	let error = $state('');

	async function create() {
		busy = true;
		error = '';
		const res = await fetch('/api/lobby', { method: 'POST' }).catch(() => null);
		busy = false;
		if (!res?.ok) return (error = 'Couldn’t open a lobby. Try again.');
		await goto(`/lobby/${(await res.json()).code}`);
	}

	function join(e: SubmitEvent) {
		e.preventDefault();
		const c = code
			.trim()
			.toUpperCase()
			.replace(/[^A-Z0-9]/g, '');
		if (c.length === 6) goto(`/lobby/${c}`);
		else error = 'Lobby codes have 6 letters and numbers.';
	}
</script>

<svelte:head><title>Draw together · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Private lobbies</span>
	<h1>Draw together</h1>
	<p class="lede">
		Up to 8 friends. <b>Competition</b>: the same picture against the clock, then a reveal and the
		grader picks the winner. <b>Free draw</b>: one shared canvas, everyone drawing live.
	</p>
	{#if error}<p class="err" role="alert">{error}</p>{/if}
	<div class="pp-bento grid">
		<Tile span={2} tone="ink" as="section" label="Host a lobby">
			<h2>Host a lobby</h2>
			{#if page.data.user}
				<p>You’ll get a link to send to friends. Guests can join without an account.</p>
				<div>
					<Button onclick={create} disabled={busy}>{busy ? 'Opening…' : 'Create a lobby'}</Button>
				</div>
			{:else}
				<p>Hosts need a free account (guests can join with a link).</p>
				<div><Button href="/login?next=/lobby">Sign in to host</Button></div>
			{/if}
		</Tile>
		<Tile span={2} as="section" label="Join a lobby">
			<h2>Join with a code</h2>
			<form onsubmit={join} class="join">
				<label>
					<span>Lobby code</span>
					<input
						bind:value={code}
						maxlength="8"
						autocomplete="off"
						spellcheck="false"
						placeholder="ABC123"
					/>
				</label>
				<Button type="submit" variant="dark">Join</Button>
			</form>
		</Tile>
	</div>
</div>

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
	}
	.lede {
		margin: 4px 0 var(--pp-gap);
		max-width: 70ch;
		color: var(--pp-ink-2);
	}
	h2 {
		font-size: 24px;
	}
	.err {
		font-weight: 600;
		color: var(--pp-accent-ink);
	}
	.join {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 10px;
	}
	.join label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
	}
	.join input {
		min-height: 48px;
		width: 160px;
		padding: 8px 14px;
		border: 2px solid var(--pp-track);
		border-radius: 14px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 700 20px var(--pp-font-display);
		letter-spacing: 2px;
		text-transform: uppercase;
	}
</style>
