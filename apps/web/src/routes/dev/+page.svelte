<script lang="ts">
	import Tile from '#lib/ui/Tile.svelte';

	const pages = [
		{
			href: '/dev/pictures',
			name: 'Review live pictures',
			blurb: 'Keep, move or reject what the picture sorter let into the live pool.'
		},
		{
			href: '/dev/references',
			name: 'Reference candidates',
			blurb: 'Approve hand-picked candidates before they go live.'
		},
		{
			href: '/dev/keypoints',
			name: 'Mark keypoints',
			blurb: 'Mark the spots students tap to check proportions.'
		},
		{ href: '/dev/rate', name: 'Rate drawings', blurb: 'Label samples to calibrate the grader.' },
		{ href: '/dev/feedback', name: 'Tip feedback', blurb: 'How students voted on tips.' }
	];
</script>

<svelte:head><title>Dev tools · PencilPilot (dev)</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Dev only</span>
	<h1>Dev tools</h1>
	<div class="pp-bento">
		{#each pages as p (p.href)}
			<Tile href={p.href} span={2}>
				<h2>{p.name}</h2>
				<p>{p.blurb}</p>
			</Tile>
		{/each}
	</div>
</div>

<style>
	h1 {
		font-size: 40px;
		margin: 4px 0 16px;
	}
	h2 {
		font-size: 24px;
	}
	p {
		margin: 0;
		color: var(--pp-ink-2);
	}
</style>
