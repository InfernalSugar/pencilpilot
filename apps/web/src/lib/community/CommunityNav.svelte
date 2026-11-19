<script lang="ts">
	let { current }: { current: 'closer' | 'rate' | 'mine' } = $props();
	const tabs = [
		{ id: 'closer', href: '/community', label: 'Which is closer?' },
		{ id: 'rate', href: '/community/rate', label: 'Rate a drawing' },
		{ id: 'mine', href: '/community/mine', label: 'My shared drawings' }
	] as const;
</script>

<nav class="tabs" aria-label="Community">
	{#each tabs as t (t.id)}
		<a href={t.href} aria-current={current === t.id ? 'page' : undefined}>{t.label}</a>
	{/each}
</nav>

<style>
	.tabs {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		padding: 4px;
		margin: 16px 0 var(--pp-gap);
		background: var(--pp-surface-sunk);
		border-radius: 999px;
		width: fit-content;
		max-width: 100%;
	}
	a {
		padding: 10px 16px;
		border-radius: 999px;
		text-decoration: none;
		font-weight: 600;
		font-size: 14px;
	}
	a[aria-current='page'] {
		background: var(--pp-ink);
		color: #fff;
	}
</style>
