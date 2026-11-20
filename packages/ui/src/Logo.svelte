<script lang="ts">
	let { size = 34 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
	<rect x="1" y="1" width="30" height="30" rx="9" fill="var(--pp-ink)" />
	<path
		d="M9 23l2-6 9-9 4 4-9 9Z"
		fill="none"
		stroke="var(--pp-ground)"
		stroke-width="2.2"
		stroke-linejoin="round"
	/>
	<circle cx="22.5" cy="9.5" r="2.5" fill="var(--pp-accent)" />
</svg>
