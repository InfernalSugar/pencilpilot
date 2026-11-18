<script lang="ts">
	import { page } from '$app/state';
	import Logo from './Logo.svelte';

	const links = [
		{ href: '/', label: 'Practice' },
		{ href: '/lessons', label: 'Lessons' },
		{ href: '/progress', label: 'Progress' }
	];
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
</header>

<style>
	.header {
		max-width: var(--pp-max);
		margin: 0 auto;
		padding: 24px var(--pp-page-pad);
		display: flex;
		flex-wrap: wrap;
		gap: 16px;
		align-items: center;
		justify-content: space-between;
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
	nav a[aria-current='page'] {
		color: var(--pp-accent-ink);
		font-weight: 700;
	}
</style>
