<script lang="ts">
	import CommunityPage from '#lib/community/CommunityPage.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
</script>

<CommunityPage {data} {form} />
