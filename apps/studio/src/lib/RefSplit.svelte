<script lang="ts">
	/**
	 * The drawing area, with the reference in a pane beside the canvas when `pane` is
	 * given (as the website does): a divider between them resizes it (drag, or the arrow
	 * keys when it has focus). The canvas stays in the same place in the page whether
	 * the pane shows or not, so showing it never starts the canvas again.
	 */
	import type { Snippet } from 'svelte';
	import { REF_SPLIT_MAX, REF_SPLIT_MIN } from './settings.svelte';

	let {
		split = $bindable(),
		pane = null,
		leftHanded = false,
		children
	}: {
		/** the pane's share of the drawing area */
		split: number;
		pane?: Snippet | null;
		/** the pane on the left, where the drawing hand doesn't cover it (left-handed: the right) */
		leftHanded?: boolean;
		children: Snippet;
	} = $props();

	let el = $state<HTMLDivElement>();
	let dragging = false;
	const set = (v: number) => (split = Math.min(REF_SPLIT_MAX, Math.max(REF_SPLIT_MIN, v)));
</script>

<div
	class="split"
	class:on={!!pane}
	class:flip={leftHanded}
	bind:this={el}
	style="--split: {split * 100}%"
>
	{#if pane}
		<div class="pane" aria-label="Reference beside the canvas" role="region">{@render pane()}</div>
		<!-- a window splitter: drag, or arrow keys when focused -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
		<div
			class="divider"
			role="separator"
			tabindex="0"
			aria-label="Resize the reference"
			aria-orientation="vertical"
			aria-valuemin={REF_SPLIT_MIN * 100}
			aria-valuemax={REF_SPLIT_MAX * 100}
			aria-valuenow={Math.round(split * 100)}
			onpointerdown={(e) => {
				e.currentTarget.setPointerCapture(e.pointerId);
				dragging = true;
			}}
			onpointermove={(e) => {
				if (!dragging || !el) return;
				const r = el.getBoundingClientRect();
				const x = (e.clientX - r.left) / r.width;
				set(leftHanded ? 1 - x : x);
			}}
			onpointerup={() => (dragging = false)}
			onpointercancel={() => (dragging = false)}
			onkeydown={(e) => {
				const step = e.shiftKey ? 0.1 : 0.02;
				const towards = leftHanded ? -1 : 1;
				if (e.key === 'Home') set(REF_SPLIT_MIN);
				else if (e.key === 'End') set(REF_SPLIT_MAX);
				else if (e.key === 'ArrowLeft') set(split - step * towards);
				else if (e.key === 'ArrowRight') set(split + step * towards);
				else return;
				e.preventDefault();
			}}
		></div>
	{/if}
	<div class="canvas-pane">{@render children()}</div>
</div>

<style>
	.split {
		position: absolute;
		inset: 0;
		display: flex;
	}
	.split.flip {
		flex-direction: row-reverse;
	}
	.canvas-pane {
		position: relative;
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
		height: 100%;
		order: 3;
	}
	.pane {
		flex: 0 0 var(--split);
		min-width: 0;
		min-height: 0;
		order: 1;
	}
	.divider {
		flex: none;
		width: 12px;
		position: relative;
		order: 2;
		cursor: col-resize;
		touch-action: none;
		background: var(--pp-surface-sunk);
	}
	.divider::after {
		content: '';
		position: absolute;
		left: 50%;
		top: 50%;
		width: 4px;
		height: 44px;
		transform: translate(-50%, -50%);
		border-radius: 4px;
		background: var(--pp-ink-2);
		opacity: 0.45;
		transition: opacity 0.15s;
	}
	.divider:hover::after,
	.divider:focus-visible::after,
	.divider:active::after {
		opacity: 1;
		background: var(--pp-accent);
	}
	.divider:focus-visible {
		outline: 2px solid var(--pp-accent);
		outline-offset: -2px;
	}
</style>
