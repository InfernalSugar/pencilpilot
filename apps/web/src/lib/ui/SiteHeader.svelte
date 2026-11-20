<script lang="ts">
	import { page } from '$app/state';
	import Logo from '@pencilpilot/ui/Logo.svelte';

	let {
		user,
		community = false
	}: { user: { id: string; name: string; email: string | null } | null; community?: boolean } =
		$props();
	const initial = $derived((user?.name || user?.email || '?').trim()[0]?.toUpperCase() ?? '?');

	const links = $derived([
		{ href: '/', label: 'Practice' },
		{ href: '/lessons', label: 'Lessons' },
		{ href: '/drills', label: 'Drills' },
		...(community ? [{ href: '/community', label: 'Community' }] : []),
		{ href: '/progress', label: 'Progress' }
	]);
	const current = (href: string) =>
		href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
</script>

<header class="header">
	<a class="brand" href="/" aria-label="PencilPilot home">
		<Logo />
		<span>PencilPilot</span>
	</a>
	<nav aria-label="Main">
		{#each links as l (l.href)}
			<a href={l.href} aria-current={current(l.href) ? 'page' : undefined}>{l.label}</a>
		{/each}
	</nav>
	{#if user}
		<a
			class="me"
			href="/account"
			aria-label="Your account"
			title={user.name || user.email || 'Account'}
		>
			{initial}
		</a>
	{:else}
		<a class="signin" href="/login">Sign in</a>
	{/if}
</header>

<style>
	.header {
		max-width: var(--pp-max);
		margin: 0 auto;
		padding: 24px var(--pp-page-pad);
		display: grid;
		grid-template-columns: auto 1fr auto;
		grid-template-areas: 'brand nav me';
		gap: 16px 28px;
		align-items: center;
	}
	.brand {
		grid-area: brand;
	}
	nav {
		grid-area: nav;
		justify-self: end;
	}
	.signin,
	.me {
		grid-area: me;
		text-decoration: none;
	}
	/* Phones: logo + account on one row, the four sections on the next, spread edge to edge. */
	@media (max-width: 640px) {
		.header {
			grid-template-columns: 1fr auto;
			grid-template-areas: 'brand me' 'nav nav';
			padding-top: 16px;
			padding-bottom: 12px;
			gap: 8px;
		}
		nav {
			justify-self: stretch;
			justify-content: space-between;
			flex-wrap: nowrap;
			gap: 4px 10px;
			font-size: 15px;
		}
		nav a {
			padding: 10px 0;
		}
		.brand {
			font-size: 21px;
		}
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 12px;
		text-decoration: none;
		font-family: var(--pp-font-display);
		font-weight: 800;
		font-size: 24px;
		letter-spacing: -0.6px;
	}
	nav {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 28px;
		font-weight: 500;
	}
	nav a {
		text-decoration: none;
		padding: 8px 0;
	}
	nav {
		align-items: center;
	}
	.signin {
		padding: 10px 18px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 600;
	}
	.signin:hover {
		color: #fff;
	}
	.me {
		width: 40px;
		height: 40px;
		display: grid;
		place-items: center;
		border-radius: 999px;
		background: var(--pp-yellow);
		font-family: var(--pp-font-display);
		font-weight: 800;
	}
	nav a[aria-current='page'] {
		color: var(--pp-accent-ink);
		font-weight: 700;
	}
</style>
