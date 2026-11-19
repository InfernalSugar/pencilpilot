<script lang="ts">
	import '#lib/ui/theme.css';
	import favicon from '#lib/assets/favicon.svg';
	import SiteHeader from '#lib/ui/SiteHeader.svelte';
	import { setSignedIn, syncNow } from '#lib/sync.js';
	import type { LayoutProps } from './$types';

	let { children, data }: LayoutProps = $props();

	// Whenever someone becomes signed in (page load, sign-in or sign-up, which are
	// client-side navigations), push this browser's history up and pull theirs down.
	let syncedFor = '';
	$effect(() => {
		const id = data.user?.id ?? '';
		setSignedIn(!!id);
		if (id && id !== syncedFor) {
			syncedFor = id;
			void syncNow();
		}
		if (!id) syncedFor = '';
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<a class="skip-link" href="#main">Skip to content</a>
<SiteHeader user={data.user} />
<main id="main" tabindex="-1">
	{@render children()}
</main>
