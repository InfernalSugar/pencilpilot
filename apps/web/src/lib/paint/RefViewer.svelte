<script lang="ts">
	/** The reference picture with its own zoom, pan and flip (it never changes the canvas). */
	let { src, alt, fill = false }: { src: string; alt: string; fill?: boolean } = $props();

	let zoom = $state(1);
	let panX = $state(0);
	let panY = $state(0);
	let flipped = $state(false);
	let box: HTMLDivElement;
	let drag: { id: number; x: number; y: number; px: number; py: number } | null = null;

	function zoomBy(f: number) {
		const z = Math.max(1, Math.min(8, zoom * f));
		panX *= z / zoom;
		panY *= z / zoom;
		zoom = z;
		if (zoom === 1) panX = panY = 0;
	}
	function reset() {
		zoom = 1;
		panX = panY = 0;
		flipped = false;
	}

	$effect(() => {
		const onWheel = (e: WheelEvent) => {
			if (!(e.ctrlKey || e.metaKey) && zoom === 1) return;
			e.preventDefault();
			if (e.ctrlKey || e.metaKey) zoomBy(Math.exp(-e.deltaY * 0.01));
			else {
				panX -= e.deltaX;
				panY -= e.deltaY;
			}
		};
		box.addEventListener('wheel', onWheel, { passive: false });
		return () => box.removeEventListener('wheel', onWheel);
	});
</script>

<div class="ref" class:fill>
	<div
		class="box"
		class:zoomed={zoom > 1}
		bind:this={box}
		role="presentation"
		ondblclick={() => (zoom > 1 ? reset() : zoomBy(2))}
		onpointerdown={(e) => {
			if (zoom === 1) return;
			box.setPointerCapture(e.pointerId);
			drag = { id: e.pointerId, x: e.clientX, y: e.clientY, px: panX, py: panY };
		}}
		onpointermove={(e) => {
			if (!drag || e.pointerId !== drag.id) return;
			panX = drag.px + e.clientX - drag.x;
			panY = drag.py + e.clientY - drag.y;
		}}
		onpointerup={() => (drag = null)}
		onpointercancel={() => (drag = null)}
	>
		<img
			class="fade-in"
			{src}
			{alt}
			draggable="false"
			style="transform: translate({panX}px, {panY}px) scale({zoom * (flipped ? -1 : 1)}, {zoom})"
		/>
	</div>
	<div class="tools" role="toolbar" aria-label="Reference view">
		<button
			type="button"
			aria-label="Zoom reference out"
			disabled={zoom <= 1}
			onclick={() => zoomBy(1 / 1.5)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Zoom reference in"
			disabled={zoom >= 8}
			onclick={() => zoomBy(1.5)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
		</button>
		<button
			type="button"
			aria-label="Flip reference"
			aria-pressed={flipped}
			onclick={() => (flipped = !flipped)}
		>
			<svg viewBox="0 0 24 24" aria-hidden="true">
				<path d="M12 3v18M8 7 3 12l5 5V7ZM16 7l5 5-5 5V7Z" />
			</svg>
		</button>
		{#if zoom > 1 || flipped}
			<button type="button" class="text" onclick={reset}>Reset</button>
		{/if}
	</div>
</div>

<style>
	.ref {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
	}
	.ref.fill {
		flex: 1;
	}
	.box {
		position: relative;
		overflow: hidden;
		border-radius: 18px;
		background: var(--pp-surface-sunk);
		touch-action: pan-y;
	}
	.fill .box {
		flex: 1;
		min-height: 120px;
	}
	.box.zoomed {
		cursor: grab;
		touch-action: none;
	}
	img {
		display: block;
		width: 100%;
		max-height: var(--ref-max-h, 320px);
		object-fit: contain;
		transition: transform 0.12s ease-out;
		user-select: none;
	}
	.fill img {
		height: 100%;
		max-height: none;
		position: absolute;
		inset: 0;
	}
	.tools {
		display: flex;
		gap: 4px;
	}
	.tools button {
		display: grid;
		place-items: center;
		min-width: 32px;
		height: 32px;
		padding: 0 8px;
		border: 0;
		border-radius: 999px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		font: 600 12px var(--pp-font-body);
		cursor: pointer;
	}
	.tools button:disabled {
		opacity: 0.35;
		cursor: default;
	}
	.tools button[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}
	svg {
		width: 16px;
		height: 16px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.fade-in {
		animation: fade 0.35s ease-out;
	}
	@keyframes fade {
		from {
			opacity: 0;
		}
	}
</style>
