<script lang="ts">
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import AuthCard from '#lib/ui/AuthCard.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import GoogleButton from '#lib/ui/GoogleButton.svelte';
	import type { PageProps } from './$types';

	let { form }: PageProps = $props();
	const next = page.url.searchParams.get('next') ?? '';
	let busy = $state(false);
</script>

<svelte:head><title>Create an account · PencilPilot</title></svelte:head>

<AuthCard eyebrow="Free account" title="Save your progress">
	{#if page.data.googleEnabled}
		<GoogleButton {next} />
		<span class="or">or with email</span>
	{/if}
	<form
		method="POST"
		use:enhance={() => {
			busy = true;
			return async ({ update }) => {
				await update();
				busy = false;
			};
		}}
	>
		{#if form?.message}<p class="error" role="alert">{form.message}</p>{/if}
		<label>
			Name (optional)<input
				name="name"
				autocomplete="nickname"
				value={form?.name ?? ''}
				maxlength="60"
			/>
		</label>
		<label>
			Email<input
				name="email"
				type="email"
				autocomplete="email"
				required
				value={form?.email ?? ''}
			/>
		</label>
		<label>
			Password<input
				name="password"
				type="password"
				autocomplete="new-password"
				minlength="8"
				required
			/>
		</label>
		<Button type="submit" disabled={busy}>{busy ? 'Creating…' : 'Create account'}</Button>
	</form>
	<p class="small">
		Already have one? <a href="/login{next ? `?next=${encodeURIComponent(next)}` : ''}">Sign in</a>
		· <a href="/privacy">Privacy</a>
	</p>

	{#snippet aside()}
		<span class="pp-eyebrow" style="color: var(--pp-lilac-ink)">Why sign up?</span>
		<ul class="why">
			<li>Your scores, streak and habits follow you to any device.</li>
			<li>Drawings you made as a guest in this browser come with you.</li>
			<li>Free. No ads. Download or delete your data any time.</li>
		</ul>
	{/snippet}
</AuthCard>

<style>
	.why {
		margin: 0;
		padding-left: 20px;
		display: flex;
		flex-direction: column;
		gap: 10px;
		font-size: 17px;
		line-height: 1.45;
	}
</style>
