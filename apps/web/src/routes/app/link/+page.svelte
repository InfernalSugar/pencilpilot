<script lang="ts">
	// Link the PencilPilot app to this account (for Draw together). A plain form, not
	// enhanced: the answer sends the browser back to the app on this computer.
	import { page } from '$app/state';
	import AuthCard from '#lib/ui/AuthCard.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const action = (name: string) => `${page.url.pathname}${page.url.search}&/${name}`;
</script>

<svelte:head><title>Link the app · PencilPilot</title></svelte:head>

<AuthCard eyebrow="PencilPilot app" title="Link this app?">
	{#if !data.ok}
		<p class="error" role="alert">
			This link didn’t come from the PencilPilot app. Start again from the app: Draw together → Sign
			in with PencilPilot.
		</p>
	{:else}
		<p class="small">
			<b>{data.label}</b> on this computer asks to use your account for Draw together. People you draw
			with will see your name, nothing else: not your email.
		</p>
		{#if form?.message}<p class="error" role="alert">{form.message}</p>{/if}
		<form method="POST" action={action('link')}>
			<label>
				Your name when drawing together<input
					name="name"
					maxlength="24"
					required
					value={(data.name ?? '').slice(0, 24)}
					autocomplete="nickname"
				/>
			</label>
			<Button type="submit">Link this PencilPilot app</Button>
		</form>
		<form method="POST" action={action('cancel')}>
			<Button type="submit" variant="outline">Not now</Button>
		</form>
		<p class="small">You can unlink it any time from your Account page.</p>
	{/if}
</AuthCard>
