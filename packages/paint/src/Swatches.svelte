<script lang="ts" module>
	export const SWATCHES = [
		'#16161a',
		'#6b6a70',
		'#b9b6ae',
		'#e8492f',
		'#3d6bff',
		'#1f9d74',
		'#f2a93b'
	];
</script>

<script lang="ts">
	/** Quick colours for the options bar (drag one onto the canvas to fill there). */
	import ColorPanel from './ColorPanel.svelte';

	let {
		color = $bindable(),
		onmore,
		moreOpen = false,
		picker = false
	}: {
		color: string;
		/** "More colours" (none: no button, unless `picker`) */
		onmore?: () => void;
		moreOpen?: boolean;
		/** the colour wheel opens the full picker in a pop-up here, not elsewhere */
		picker?: boolean;
	} = $props();

	/** the picker pop-up is open */
	let popOpen = $state(false);
	let wrap = $state<HTMLElement>();
	function outside(e: PointerEvent) {
		if (popOpen && wrap && !wrap.contains(e.target as Node)) popOpen = false;
	}
	function escape(e: KeyboardEvent) {
		if (popOpen && e.key === 'Escape') {
			e.stopPropagation();
			popOpen = false;
		}
	}

	/**
	 * ColorDrop takes a press and hold before the drag: a tap picks the colour even when
	 * a pen wobbles as it touches (the browser's own drag started on any small move, so a
	 * tap with a pen dragged the colour instead of choosing it).
	 */
	const HOLD_MS = 400;
	/** moving further than this before the hold is up: not a ColorDrop */
	const SLOP_PX = 12;
	let press: {
		c: string;
		id: number;
		x: number;
		y: number;
		timer: ReturnType<typeof setTimeout>;
	} | null = null;
	/** the colour lifted by a hold, following the pointer */
	let lifted = $state<{ c: string; x: number; y: number } | null>(null);
	/** a hold ended, or the press slid away: its click doesn't pick */
	let swallowClick = false;

	function down(e: PointerEvent, c: string) {
		if (e.button !== 0 || press) return;
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture?.(e.pointerId);
		press = {
			c,
			id: e.pointerId,
			x: e.clientX,
			y: e.clientY,
			timer: setTimeout(() => {
				if (press) lifted = { c: press.c, x: press.x, y: press.y };
			}, HOLD_MS)
		};
	}
	function move(e: PointerEvent) {
		if (!press || e.pointerId !== press.id) return;
		if (lifted) lifted = { ...lifted, x: e.clientX, y: e.clientY };
		else if (Math.hypot(e.clientX - press.x, e.clientY - press.y) > SLOP_PX) {
			// slid away before the hold: neither a tap nor a drop
			clearTimeout(press.timer);
			press = null;
			swallowClick = true;
		}
	}
	function up(e: PointerEvent) {
		if (!press || e.pointerId !== press.id) return;
		clearTimeout(press.timer);
		const drop = lifted;
		press = null;
		lifted = null;
		if (!drop) return;
		swallowClick = true;
		// over something that takes a colour (the canvas): a drop there, as a drag would
		const target = document.elementFromPoint(e.clientX, e.clientY);
		if (!target || (e.currentTarget as HTMLElement).contains(target)) return;
		const dt = new DataTransfer();
		dt.setData('application/x-pp-color', drop.c);
		const at = { bubbles: true, cancelable: true, clientX: e.clientX, clientY: e.clientY };
		target.dispatchEvent(new DragEvent('dragover', { ...at, dataTransfer: dt }));
		target.dispatchEvent(new DragEvent('drop', { ...at, dataTransfer: dt }));
	}
	function cancel(e: PointerEvent) {
		if (!press || e.pointerId !== press.id) return;
		clearTimeout(press.timer);
		press = null;
		lifted = null;
	}
	function pick(c: string) {
		if (swallowClick) {
			swallowClick = false;
			return;
		}
		color = c;
	}
</script>

<div class="swatches" role="radiogroup" aria-label="Colour">
	{#each SWATCHES as c (c)}
		<button
			type="button"
			role="radio"
			aria-checked={color === c}
			aria-label="Colour {c}"
			title="Tap to choose; press and hold, then drag onto the drawing to fill there"
			draggable="false"
			class="swatch"
			style="background: {c}"
			onpointerdown={(e) => down(e, c)}
			onpointermove={move}
			onpointerup={up}
			onpointercancel={cancel}
			oncontextmenu={(e) => e.preventDefault()}
			onclick={() => pick(c)}
		></button>
	{/each}
	{#if lifted}
		<div
			class="lifted"
			aria-hidden="true"
			style="left: {lifted.x}px; top: {lifted.y}px; background: {lifted.c}"
		></div>
	{/if}
	{#if picker}
		<span class="pop-anchor" bind:this={wrap}>
			<button
				type="button"
				class="swatch custom"
				aria-label="More colours"
				aria-expanded={popOpen}
				aria-haspopup="dialog"
				title="Colour wheel, recent colours and palettes"
				onclick={() => (popOpen = !popOpen)}
			></button>
			{#if popOpen}
				<div class="pop">
					<ColorPanel bind:color onclose={() => (popOpen = false)} />
				</div>
			{/if}
		</span>
	{:else if onmore}
		<button
			type="button"
			class="swatch custom"
			aria-label="More colours"
			aria-expanded={moreOpen}
			title="Colour wheel, recent colours and palettes"
			onclick={onmore}
		></button>
	{/if}
</div>
<svelte:window onpointerdown={outside} onkeydowncapture={escape} />

<style>
	.swatches {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.pop-anchor {
		position: relative;
		display: inline-flex;
	}
	/* the full picker, under the colour wheel, over the canvas */
	.pop {
		position: absolute;
		top: calc(100% + 10px);
		left: 50%;
		transform: translateX(-50%);
		z-index: 60;
		width: 300px;
		max-height: min(560px, 70vh);
		overflow: auto;
		padding: 12px;
		border-radius: 20px;
		background: var(--pp-surface, #fff);
		border: 1px solid var(--pp-surface-sunk, #e4e0d6);
		box-shadow: 0 16px 40px rgb(22 22 26 / 0.2);
	}
	.swatch {
		flex: none;
		width: 30px;
		height: 30px;
		border-radius: 999px;
		border: 2px solid #2a2a30;
		cursor: pointer;
		padding: 0;
		/* (a finger or pen held on it doesn't scroll, select or open a menu) */
		touch-action: none;
		user-select: none;
		-webkit-user-select: none;
		-webkit-touch-callout: none;
	}
	.lifted {
		position: fixed;
		z-index: 1000;
		width: 34px;
		height: 34px;
		margin: -17px 0 0 -17px;
		border-radius: 999px;
		border: 3px solid #fff;
		box-shadow: 0 2px 10px rgb(0 0 0 / 0.45);
		pointer-events: none;
	}
	.swatch[aria-checked='true'] {
		outline: 3px solid var(--pp-yellow);
		outline-offset: 1px;
	}
	.custom {
		background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
	}
</style>
