<script lang="ts">
	import Tile from '#lib/ui/Tile.svelte';
	import Button from '#lib/ui/Button.svelte';
</script>

<svelte:head><title>Progress · PencilPilot</title></svelte:head>

<div class="pp-page">
	<div class="pp-bento">
		<Tile span={3} as="section">
			<span class="pp-eyebrow">Coming in M6</span>
			<h1 style="font-size: clamp(44px, 6vw, 72px); line-height: 0.95;">Progress</h1>
			<p style="margin: 0; font-size: 18px; line-height: 1.55; color: var(--pp-ink-2);">
				Your scores over time, skills radar, streaks and recent drawings.
			</p>
		</Tile>
		<Tile tone="yellow">
			<span class="pp-eyebrow" style="color: var(--pp-ink);">Meanwhile</span>
			<Button href="/draw" variant="dark">Open the canvas</Button>
		</Tile>
	</div>
</div>
