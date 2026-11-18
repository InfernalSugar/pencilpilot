<script lang="ts">
	import '#lib/ui/theme.css';
	import favicon from '#lib/assets/favicon.svg';
	import SiteHeader from '#lib/ui/SiteHeader.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<SiteHeader />
<main>
	{@render children()}
</main>
