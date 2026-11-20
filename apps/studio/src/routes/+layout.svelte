<script lang="ts">
	import '@pencilpilot/ui/theme.css';

	let { children } = $props();
</script>

{@render children()}

<style>
	:global(html, body) {
		margin: 0;
		height: 100%;
		overflow: hidden;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font-family: var(--pp-font-body);
		/* an app, not a page: no text selection or callouts while drawing */
		-webkit-user-select: none;
		user-select: none;
		-webkit-touch-callout: none;
	}
	:global(input, textarea) {
		-webkit-user-select: text;
		user-select: text;
	}
</style>
