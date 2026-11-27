<script lang="ts" module>
	/** One line of a right-click menu: a heading, a divider, or something to do. */
	export type MenuItem =
		| { head: string }
		| { sep: true }
		| {
				label: string;
				/** a shortcut or a value, on the right */
				hint?: string;
				run: () => void;
				disabled?: boolean;
				/** a choice that is on (✓) or off */
				checked?: boolean;
				/** deletes or takes something away */
				danger?: boolean;
				/** keep the menu open after (a choice among several) */
				stay?: boolean;
		  };

	/** where a menu opens, and what's in it */
	export interface MenuAt {
		x: number;
		y: number;
		label: string;
		items: MenuItem[];
	}
</script>

<script lang="ts">
	/**
	 * A right-click menu (the new layout's): a dark card at the pointer, kept inside the
	 * window. Arrow keys move between its lines, Enter picks, Esc or a press outside closes.
	 */
	import { onMount, tick } from 'svelte';

	let { menu, onclose }: { menu: MenuAt; onclose: () => void } = $props();

	let el = $state<HTMLDivElement>();
	let left = $state(0);
	let top = $state(0);

	function place() {
		if (!el) return;
		const r = el.getBoundingClientRect();
		left = Math.max(8, Math.min(menu.x, window.innerWidth - r.width - 8));
		top = Math.max(8, Math.min(menu.y, window.innerHeight - r.height - 8));
	}
	$effect(() => {
		void menu;
		left = menu.x;
		top = menu.y;
		void tick().then(place);
	});

	const buttons = () => [
		...(el?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])
	];
	onMount(() => {
		void tick().then(() => buttons()[0]?.focus({ preventScroll: true }));
	});

	function key(e: KeyboardEvent) {
		const list = buttons();
		const i = list.indexOf(document.activeElement as HTMLButtonElement);
		if (e.key === 'Escape') {
			e.preventDefault();
			e.stopPropagation();
			onclose();
		} else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			e.preventDefault();
			e.stopPropagation();
			const by = e.key === 'ArrowDown' ? 1 : -1;
			list[(i + by + list.length) % list.length]?.focus();
		} else if (e.key === 'Tab') onclose();
	}

	function pick(it: Extract<MenuItem, { run: () => void }>) {
		if (it.disabled) return;
		if (!it.stay) onclose();
		it.run();
	}
</script>

<svelte:window
	onpointerdowncapture={(e) => {
		if (el && !el.contains(e.target as Node)) onclose();
	}}
	onresize={onclose}
/>

<!-- svelte-ignore a11y_interactive_supports_focus -->
<div
	bind:this={el}
	class="ctx-menu"
	role="menu"
	aria-label={menu.label}
	style="left: {left}px; top: {top}px"
	onkeydown={key}
	oncontextmenu={(e) => e.preventDefault()}
>
	{#each menu.items as it, i (i)}
		{#if 'head' in it}
			<span class="head">{it.head}</span>
		{:else if 'sep' in it}
			<span class="sep" aria-hidden="true"></span>
		{:else if it.checked !== undefined}
			<button
				type="button"
				role="menuitemcheckbox"
				aria-checked={it.checked}
				class:danger={it.danger}
				disabled={it.disabled}
				onclick={() => pick(it)}
			>
				<span>{it.label}</span><span class="hint">{it.checked ? '✓' : ''}{it.hint ?? ''}</span>
			</button>
		{:else}
			<button
				type="button"
				role="menuitem"
				class:danger={it.danger}
				disabled={it.disabled}
				onclick={() => pick(it)}
			>
				<span>{it.label}</span>{#if it.hint}<span class="hint">{it.hint}</span>{/if}
			</button>
		{/if}
	{/each}
</div>

<style>
	.ctx-menu {
		position: fixed;
		z-index: 200;
		display: flex;
		flex-direction: column;
		min-width: 220px;
		max-width: 320px;
		max-height: calc(100vh - 16px);
		overflow-y: auto;
		padding: 6px;
		border: 1px solid #34343c;
		border-radius: 16px;
		background: #1e1e24;
		box-shadow: 0 16px 40px rgba(22, 22, 26, 0.35);
		color: #f6f4ef;
		font: 500 13px var(--pp-font-body);
	}
	.head {
		padding: 8px 10px 4px;
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.6px;
		text-transform: uppercase;
		color: #a9a8ae;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.sep {
		height: 1px;
		margin: 4px 8px;
		background: #34343c;
	}
	button {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		min-height: 32px;
		padding: 0 10px;
		border: 0;
		border-radius: 9px;
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	button:hover:not(:disabled),
	button:focus-visible {
		background: #2a2a30;
		outline: none;
	}
	button:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	button.danger {
		color: #ff8a73;
	}
	.hint {
		flex: none;
		color: #a9a8ae;
		font-size: 12px;
	}
</style>
