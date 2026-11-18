<script lang="ts">
	import { enhance } from '$app/forms';
	import { invalidateAll } from '$app/navigation';
	import { onMount } from 'svelte';
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
	import { syncNow } from '#lib/sync.js';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let syncState = $state<'idle' | 'syncing' | 'done' | 'error'>('idle');
	let confirmDelete = $state('');

	async function sync() {
		syncState = 'syncing';
		const ok = await syncNow();
		syncState = ok ? 'done' : 'error';
		if (ok) await invalidateAll(); // refresh the saved counts
	}
	onMount(sync);
	const since = $derived(new Date(data.account.created).toLocaleDateString());
</script>

<svelte:head><title>Account · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Account</span>
	<h1>{data.account.name || 'Your account'}</h1>
	{#if data.welcome}
		<p class="welcome" role="status">
			Welcome! Your guest drawings from this browser are now saved to your account.
		</p>
	{/if}

	<div class="pp-bento grid">
		<Tile tone="ink">
			<span class="pp-eyebrow on-ink">Saved drawings</span>
			<span class="big">{data.counts.attempts}</span>
			<span class="note">{data.counts.drills} drill sessions · member since {since}</span>
		</Tile>
		<Tile tone="yellow">
			<span class="pp-eyebrow ink">Sync</span>
			<span class="mid">
				{syncState === 'syncing'
					? 'Syncing…'
					: syncState === 'done'
						? 'Up to date'
						: syncState === 'error'
							? 'Couldn’t sync'
							: 'Ready'}
			</span>
			<div>
				<Button variant="dark" onclick={sync} disabled={syncState === 'syncing'}>Sync now</Button>
			</div>
		</Tile>
		<Tile span={2}>
			<h2>Profile</h2>
			<p class="muted">
				{data.account.email ?? 'No email'} · signs in with
				{[data.account.hasPassword && 'password', data.account.hasGoogle && 'Google']
					.filter(Boolean)
					.join(' and ')}
			</p>
			<form method="POST" action="?/rename" use:enhance class="row">
				<label class="grow">
					Display name<input name="name" value={data.account.name} maxlength="60" />
				</label>
				<Button type="submit" variant="outline">Save</Button>
			</form>
			{#if form?.saved === 'name'}<p class="ok" role="status">Saved.</p>{/if}
		</Tile>

		<Tile span={2}>
			<h2>{data.account.hasPassword ? 'Change password' : 'Add a password'}</h2>
			<form method="POST" action="?/password" use:enhance>
				{#if data.account.hasPassword}
					<label>
						Current password<input
							name="current"
							type="password"
							autocomplete="current-password"
							required
						/>
					</label>
				{/if}
				<label>
					New password<input
						name="new"
						type="password"
						autocomplete="new-password"
						minlength="8"
						required
					/>
				</label>
				<div><Button type="submit" variant="outline">Update password</Button></div>
			</form>
			{#if form?.form === 'password' && form?.message}<p class="error" role="alert">
					{form.message}
				</p>{/if}
			{#if form?.saved === 'password'}<p class="ok" role="status">
					Password updated. Other devices were signed out.
				</p>{/if}
		</Tile>

		<Tile span={2}>
			<h2>Your data</h2>
			<p class="muted">Everything we store about you: your email, name and practice history.</p>
			<div class="row">
				<Button href="/account/export" variant="outline">Download my data</Button>
				<form method="POST" action="/logout">
					<Button type="submit" variant="dark">Sign out</Button>
				</form>
			</div>
		</Tile>

		<Tile span={2}>
			<h2>Delete account</h2>
			<p class="muted">
				This permanently deletes your account and all saved progress. It can’t be undone.
			</p>
			<form method="POST" action="?/delete" use:enhance class="row">
				<label class="grow">
					Type DELETE to confirm<input
						name="confirm"
						bind:value={confirmDelete}
						autocomplete="off"
					/>
				</label>
				<Button type="submit" disabled={confirmDelete !== 'DELETE'}>Delete forever</Button>
			</form>
			{#if form?.form === 'delete' && form?.message}<p class="error" role="alert">
					{form.message}
				</p>{/if}
		</Tile>
	</div>
</div>

<style>
	h1 {
		font-size: clamp(40px, 6vw, 64px);
		line-height: 0.95;
		margin: 4px 0 16px;
	}
	h2 {
		font-size: 22px;
		font-weight: 700;
	}
	.welcome {
		margin: 0 0 16px;
		padding: 14px 18px;
		border-radius: 16px;
		background: var(--pp-lilac);
		font-weight: 600;
	}
	.grid {
		grid-auto-rows: minmax(160px, auto);
	}
	.big {
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 72px;
		line-height: 0.85;
		margin-top: auto;
	}
	.mid {
		font-family: var(--pp-font-display);
		font-weight: 700;
		font-size: 26px;
		margin-top: auto;
	}
	.on-ink {
		color: var(--pp-on-ink-muted);
	}
	.ink {
		color: var(--pp-ink);
	}
	.note {
		font-size: 14px;
		opacity: 0.85;
	}
	.muted {
		margin: 0;
		color: var(--pp-muted);
	}
	form {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.row {
		flex-direction: row;
		flex-wrap: wrap;
		align-items: flex-end;
		gap: 12px;
		display: flex;
	}
	.grow {
		flex: 1 1 200px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 6px;
		font-weight: 600;
		font-size: 14px;
	}
	input {
		padding: 12px 14px;
		border: 2px solid var(--pp-track);
		border-radius: 12px;
		font: 500 15px var(--pp-font-body);
		background: var(--pp-ground);
	}
	.error {
		margin: 0;
		color: var(--pp-accent-ink);
		font-weight: 600;
	}
	.ok {
		margin: 0;
		color: var(--pp-cat-env);
		font-weight: 600;
	}
</style>
