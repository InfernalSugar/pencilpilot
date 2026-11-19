<script lang="ts" module>
	export type ToolId =
		| 'brush'
		| 'eraser'
		| 'picker'
		| 'fill'
		| 'select-rect'
		| 'select-lasso'
		| 'select-wand'
		| 'transform'
		| 'liquify'
		| 'gradient'
		| 'adjust';
	export const ALL_TOOLS: ToolId[] = [
		'brush',
		'eraser',
		'picker',
		'fill',
		'select-rect',
		'select-lasso',
		'select-wand',
		'transform',
		'liquify',
		'gradient',
		'adjust'
	];
	export const SELECT_TOOLS: { id: ToolId; label: string; key: string; icon: string }[] = [
		{ id: 'select-rect', label: 'Rectangle select (M)', key: 'm', icon: 'M4 4h16v16H4Z' },
		{
			id: 'select-lasso',
			label: 'Lasso select (L)',
			key: 'l',
			icon: 'M7 20c-2 0-3-1-3-2.5S6 15 8 15c6 0 12-2 12-7 0-3-3-5-8-5S3 5 3 9c0 2 2 4 5 6'
		},
		{
			id: 'select-wand',
			label: 'Magic wand (W)',
			key: 'w',
			icon: 'm4 20 11-11M15 4v3M19 8h3M18 5l2-2M11 6 9 4'
		}
	];
</script>

<script lang="ts">
	/** The studio's tool strip: the tools a screen offers, plus size and opacity. */
	let {
		tools = ALL_TOOLS,
		tool = $bindable('brush'),
		eraser = $bindable(false),
		size = $bindable(1),
		opacity = $bindable(1),
		adjustOpen = false,
		onadjust,
		showOpacity = true
	}: {
		tools?: ToolId[];
		/** the active tool (the canvas's tool name; 'brush' covers the eraser too) */
		tool?: string;
		eraser?: boolean;
		size?: number;
		opacity?: number;
		adjustOpen?: boolean;
		onadjust?: () => void;
		/** some screens (drills) always draw at full opacity */
		showOpacity?: boolean;
	} = $props();

	const has = (t: ToolId) => tools.includes(t);
</script>

<div class="rail-inner" role="toolbar" aria-label="Drawing tools">
	<div class="rail-tools" class:single={tools.length <= 4}>
		{#if has('brush')}
			<button
				type="button"
				class="tool"
				aria-pressed={!eraser && tool === 'brush'}
				aria-label="Brush (B)"
				title="Brush (B)"
				onclick={() => {
					eraser = false;
					tool = 'brush';
				}}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
				</svg>
			</button>
		{/if}
		{#if has('eraser')}
			<button
				type="button"
				class="tool"
				aria-pressed={eraser && tool === 'brush'}
				aria-label="Eraser (E)"
				title="Eraser (E) — uses the current brush shape"
				onclick={() => {
					eraser = true;
					tool = 'brush';
				}}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="m7 21-4-4L14 6l5 5-8 10Z" /><path d="M22 21H7" />
				</svg>
			</button>
		{/if}
		{#if has('picker')}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'picker'}
				aria-label="Eyedropper (I)"
				title="Eyedropper (I): pick a colour from the canvas. Alt+click works too."
				onclick={() => (tool = tool === 'picker' ? 'brush' : 'picker')}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="m2 22 1-4 9-9 3 3-9 9Z" /><path d="m14 7 3-3a2.1 2.1 0 0 1 3 3l-3 3" /><path
						d="m11 6 7 7"
					/>
				</svg>
			</button>
		{/if}
		{#if has('fill')}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'fill'}
				aria-label="Fill (G)"
				title="Fill (G): fill an area with the colour. Or drag a colour swatch onto the canvas."
				onclick={() => (tool = tool === 'fill' ? 'brush' : 'fill')}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.3"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="m19 11-8-8-8.5 8.5a2.1 2.1 0 0 0 0 3L8 20a2.1 2.1 0 0 0 3 0Z" /><path
						d="m5 2 5 5"
					/><path d="M2 13h15" /><path d="M22 20a2 2 0 1 1-4 0c0-1.6 2-3 2-3s2 1.4 2 3Z" />
				</svg>
			</button>
		{/if}
		{#if SELECT_TOOLS.some((t) => has(t.id))}
			{#each SELECT_TOOLS.filter((t) => has(t.id)) as t (t.id)}
				<button
					type="button"
					class="tool"
					aria-pressed={tool === t.id}
					aria-label={t.label}
					title="{t.label}. Shift adds, Alt subtracts."
					onclick={() => (tool = tool === t.id ? 'brush' : t.id)}
				>
					<svg
						width="22"
						height="22"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.1"
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-dasharray={t.id === 'select-wand' ? undefined : '3 2.5'}
						aria-hidden="true"
					>
						<path d={t.icon} />
					</svg>
				</button>
			{/each}
		{/if}
		{#if has('transform')}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'transform'}
				aria-label="Transform (V)"
				title="Transform (V): move, scale, rotate or flip the selection (or the whole layer)"
				onclick={() => (tool = tool === 'transform' ? 'brush' : 'transform')}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.1"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path
						d="M12 2v20M2 12h20M12 2l-3 3M12 2l3 3M12 22l-3-3M12 22l3-3M2 12l3-3M2 12l3 3M22 12l-3-3M22 12l-3 3"
					/>
				</svg>
			</button>
		{/if}
		{#if has('liquify')}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'liquify'}
				aria-label="Liquify (K)"
				title="Liquify (K): push parts of the layer into place"
				onclick={() => (tool = tool === 'liquify' ? 'brush' : 'liquify')}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.1"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<path d="M3 12c3-6 6 6 9 0s6 6 9 0" /><path d="M3 18c3-6 6 6 9 0s6 6 9 0" />
				</svg>
			</button>
		{/if}
		{#if has('gradient')}
			<button
				type="button"
				class="tool"
				aria-pressed={tool === 'gradient'}
				aria-label="Gradient"
				title="Gradient: drag across the canvas"
				onclick={() => (tool = tool === 'gradient' ? 'brush' : 'gradient')}
			>
				<svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true">
					<defs>
						<linearGradient id="gr-ic" x1="0" x2="1">
							<stop offset="0" stop-color="currentColor" />
							<stop offset="1" stop-color="currentColor" stop-opacity="0" />
						</linearGradient>
					</defs>
					<rect
						x="3"
						y="5"
						width="18"
						height="14"
						rx="3"
						fill="url(#gr-ic)"
						stroke="currentColor"
						stroke-width="2"
					/>
				</svg>
			</button>
		{/if}
		{#if has('adjust')}
			<button
				type="button"
				class="tool"
				aria-pressed={adjustOpen}
				aria-expanded={adjustOpen}
				aria-label="Adjust"
				title="Adjust the layer: colour, brightness, blur"
				onclick={() => onadjust?.()}
			>
				<svg
					width="22"
					height="22"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2.1"
					stroke-linecap="round"
					aria-hidden="true"
				>
					<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" /><circle
						cx="16"
						cy="6"
						r="2"
					/><circle cx="10" cy="12" r="2" /><circle cx="18" cy="18" r="2" />
				</svg>
			</button>
		{/if}
	</div>
	<div class="rail-sliders">
		<label class="slider">
			<span>Size <b>{Math.round(size * 100)}%</b></span>
			<input
				type="range"
				min="-2"
				max="2.6"
				step="0.05"
				value={Math.log2(size)}
				oninput={(e) => (size = +(2 ** +e.currentTarget.value).toFixed(2))}
			/>
		</label>
		{#if showOpacity}
			<label class="slider">
				<span>Opacity <b>{Math.round(opacity * 100)}%</b></span>
				<input type="range" min="0.05" max="1" step="0.05" bind:value={opacity} />
			</label>
		{/if}
	</div>
</div>

<style>
	.rail-inner {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.rail-tools {
		display: grid;
		grid-template-columns: repeat(2, 44px);
		gap: 6px;
	}
	.rail-tools.single {
		grid-template-columns: 44px;
		justify-content: center;
	}
	.tool {
		width: 44px;
		height: 44px;
		padding: 0;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: #2a2a30;
		color: var(--pp-on-ink);
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.tool[aria-pressed='true'] {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.rail-sliders {
		display: flex;
		justify-content: space-around;
		gap: 4px;
		padding-top: 8px;
		border-top: 1px solid #34343c;
	}
	.slider {
		display: flex;
		flex-direction: column;
		gap: 4px;
		width: 44px;
		align-items: center;
		text-align: center;
		font-size: 11px;
		color: var(--pp-on-ink-muted);
	}
	.slider b {
		color: var(--pp-on-ink);
	}
	.slider input {
		accent-color: var(--pp-yellow);
		writing-mode: vertical-lr;
		direction: rtl;
		width: 22px;
		height: 120px;
	}
	@media (max-width: 700px) {
		.rail-inner {
			gap: 8px;
		}
		.rail-tools,
		.rail-tools.single {
			display: flex;
			gap: 6px;
			overflow-x: auto;
			scrollbar-width: none;
		}
		.tool {
			flex: none;
		}
		.rail-sliders {
			order: -1;
			gap: 12px;
			padding: 0 4px;
			border: 0;
		}
		.slider {
			flex: 1 1 0;
			width: auto;
			align-items: stretch;
			text-align: left;
		}
		.slider input {
			writing-mode: horizontal-tb;
			direction: ltr;
			width: auto;
			height: auto;
		}
	}
</style>
