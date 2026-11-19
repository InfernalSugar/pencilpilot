<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';

	type Variant = 'primary' | 'dark' | 'outline' | 'soft';

	let {
		variant = 'primary',
		href,
		children,
		...rest
	}: {
		variant?: Variant;
		href?: string;
		children: Snippet;
	} & HTMLButtonAttributes = $props();
</script>

{#if href}
	<a class="btn {variant}" {href}>{@render children()}</a>
{:else}
	<button class="btn {variant}" type="button" {...rest}>{@render children()}</button>
{/if}

<style>
	.btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		min-height: 48px;
		padding: 14px 24px;
		border-radius: var(--pp-radius-btn);
		border: 2px solid transparent;
		font: 600 17px/1 var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
		transition:
			transform 0.2s var(--pp-spring),
			background 0.15s;
	}
	.btn:hover {
		transform: translateY(-2px);
	}
	.btn:active {
		transform: translateY(1px);
	}
	.btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
		transform: none;
	}
	.primary {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.primary:hover {
		color: #fff;
	}
	.dark {
		background: var(--pp-ink);
		color: #fff;
	}
	.dark:hover {
		color: #fff;
	}
	.outline {
		background: transparent;
		border-color: var(--pp-ink);
		color: var(--pp-ink);
	}
	.soft {
		background: var(--pp-ground);
		color: var(--pp-ink);
	}
</style>
