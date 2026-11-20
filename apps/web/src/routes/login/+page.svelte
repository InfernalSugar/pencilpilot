<script lang="ts">
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import AuthCard from '#lib/ui/AuthCard.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import GoogleButton from '#lib/ui/GoogleButton.svelte';
	import type { PageProps } from './$types';

	let { form, data }: PageProps = $props();
	const next = page.url.searchParams.get('next') ?? '';
	let busy = $state(false);
</script>

<svelte:head><title>Sign in · PencilPilot</title></svelte:head>

<AuthCard eyebrow="Welcome back" title="Sign in">
	{#if data.error === 'google'}<p class="error" role="alert">
			Google sign-in didn’t work. Please try again.
		</p>{/if}
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
			Email<input
				name="email"
				type="email"
				autocomplete="email"
				required
				value={form?.email ?? ''}
			/>
		</label>
		<label>
			Password<input name="password" type="password" autocomplete="current-password" required />
		</label>
		<Button type="submit" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</Button>
	</form>
	<p class="small">
		New here? <a href="/signup{next ? `?next=${encodeURIComponent(next)}` : ''}">
			Create a free account
		</a>
	</p>
	<p class="small">Forgot your password? Password reset by email is coming soon.</p>
</AuthCard>
