<script lang="ts">
	import DrillSession from '#lib/drill/DrillSession.svelte';
	import { drillById } from '#lib/drills.js';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const drill = $derived(drillById(data.id)!);
</script>

{#key drill.id}
	<DrillSession {drill} />
{/key}
