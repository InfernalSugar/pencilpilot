<script lang="ts" module>
	export interface StudioTab {
		id: string;
		label: string;
		/** small second line (e.g. the current layer or brush) */
		meta?: string;
	}
</script>

<script lang="ts">
	/**
	 * The drawing studio (design A): a top bar, a tool strip on the left, an
	 * options bar over the canvas, the canvas, and a dock on the right. Tablets
	 * get the dock as a slide-over, phones as a bottom sheet with the tools
	 * along the bottom. Every drawing screen uses it so they all work alike.
	 */
	import { onMount, type Snippet } from 'svelte';

	let {
		backHref,
		title,
		actions,
		rail,
		options,
		context,
		banner,
		stage,
		dockTop,
		panel,
		dockBottom,
		after,
		tabs = [],
		tab = $bindable(''),
		dockOpen = $bindable(false),
		dockLabel = 'Panel'
	}: {
		/** the back button (none: no back button) */
		backHref?: string;
		title: Snippet;
		/** right side of the top bar */
		actions?: Snippet;
		rail: Snippet;
		/** the dark options bar above the canvas */
		options?: Snippet;
		/** the current tool's options, under the options bar */
		context?: Snippet;
		/** a note above the canvas */
		banner?: Snippet;
		stage: Snippet;
		/** top of the dock (reference, players…) */
		dockTop?: Snippet;
		/** the dock's tab content */
		panel?: Snippet<[string]>;
		dockBottom?: Snippet;
		/** floating things (windows) outside the layout */
		after?: Snippet;
		tabs?: StudioTab[];
		tab?: string;
		dockOpen?: boolean;
		/** the dock's button when it has no tabs */
		dockLabel?: string;
	} = $props();

	let root: HTMLDivElement;
	let height = $state('calc(100dvh - 80px)');
	/** focus mode: the site header is hidden for more drawing room */
	let focus = $state(false);

	function fit() {
		if (!root) return;
		const top = root.getBoundingClientRect().top + window.scrollY;
		height = `calc(100dvh - ${Math.max(0, Math.round(top))}px)`;
	}

	function setFocus(on: boolean) {
		focus = on;
		document.documentElement.classList.toggle('pp-focus', on);
		try {
			localStorage.setItem('pp.focus', JSON.stringify(on));
		} catch {
			/* storage blocked */
		}
		requestAnimationFrame(fit);
	}

	onMount(() => {
		try {
			focus = JSON.parse(localStorage.getItem('pp.focus') ?? 'false') === true;
		} catch {
			focus = false;
		}
		document.documentElement.classList.toggle('pp-focus', focus);
		fit();
		requestAnimationFrame(fit);
		window.addEventListener('resize', fit);
		return () => {
			window.removeEventListener('resize', fit);
			document.documentElement.classList.remove('pp-focus');
		};
	});

	function toggleDock(id: string) {
		if (dockOpen && (tab === id || !tabs.length)) dockOpen = false;
		else {
			if (id) tab = id;
			dockOpen = true;
		}
	}

	const hasDock = $derived(!!(dockTop || panel));
</script>

<div class="studio" bind:this={root} style="height: {height}">
	<header class="sbar">
		{#if backHref}
			<a class="icon-btn" href={backHref} aria-label="Back" title="Back">
				<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 5l-7 7 7 7" /></svg>
			</a>
		{/if}
		<div class="title">{@render title()}</div>
		<div class="sbar-actions">
			{@render actions?.()}
			{#if hasDock}
				{#if tabs.length}
					{#each tabs.filter((t) => t.id !== 'colour') as t (t.id)}
						<button
							type="button"
							class="icon-btn dock-btn"
							aria-label={t.label}
							aria-pressed={dockOpen && tab === t.id}
							title={t.label}
							onclick={() => toggleDock(t.id)}
						>
							{#if t.id === 'brushes'}
								<svg viewBox="0 0 24 24" aria-hidden="true">
									<path d="M9.5 14.5 19 5a2 2 0 0 1 3 3l-9.5 9.5" /><path
										d="M7 16c-2 0-3 1.5-3 3.5 0 .8-.6 1.5-1.5 1.5H7a4 4 0 0 0 4-4c0-1.3-1-2-2-2Z"
									/>
								</svg>
							{:else}
								<svg viewBox="0 0 24 24" aria-hidden="true">
									<path d="m12 3 9 5-9 5-9-5Z" /><path d="m3 13 9 5 9-5" />
								</svg>
							{/if}
						</button>
					{/each}
				{:else}
					<button
						type="button"
						class="icon-btn dock-btn"
						aria-label={dockLabel}
						aria-pressed={dockOpen}
						title={dockLabel}
						onclick={() => toggleDock('')}
					>
						<svg viewBox="0 0 24 24" aria-hidden="true">
							<rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" />
						</svg>
					</button>
				{/if}
			{/if}
			<button
				type="button"
				class="icon-btn focus-btn"
				aria-pressed={focus}
				aria-label={focus ? 'Show the site menu' : 'Hide the site menu for more room'}
				title={focus ? 'Show the site menu' : 'More room: hide the site menu'}
				onclick={() => setFocus(!focus)}
			>
				{#if focus}
					<svg viewBox="0 0 24 24" aria-hidden="true">
						<path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" />
					</svg>
				{:else}
					<svg viewBox="0 0 24 24" aria-hidden="true">
						<path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
					</svg>
				{/if}
			</button>
		</div>
	</header>

	<div class="work">
		<div class="rail">{@render rail()}</div>

		<div class="center">
			{#if options}
				<div class="opts" role="group" aria-label="Brush options">{@render options()}</div>
			{/if}
			{#if context}
				<div class="ctx">{@render context()}</div>
			{/if}
			{@render banner?.()}
			<div class="stage">{@render stage()}</div>
		</div>

		{#if hasDock}
			{#if dockOpen}
				<button
					type="button"
					class="scrim"
					aria-label="Close the panel"
					onclick={() => (dockOpen = false)}
				></button>
			{/if}
			<aside class="dock" class:open={dockOpen} aria-label="Side panel">
				<button
					type="button"
					class="dock-close"
					aria-label="Close the panel"
					onclick={() => (dockOpen = false)}
				>
					×
				</button>
				{#if dockTop}<div class="dock-top">{@render dockTop()}</div>{/if}
				{#if panel && tabs.length}
					{#if tabs.length > 1}
						<div class="tabs" role="tablist" aria-label="Panels">
							{#each tabs as t (t.id)}
								<button
									type="button"
									role="tab"
									id="tab-{t.id}"
									aria-selected={tab === t.id}
									aria-controls="panel-{t.id}"
									class="tab"
									onclick={() => (tab = t.id)}
								>
									{t.label}
									{#if t.meta}<span class="tab-meta">{t.meta}</span>{/if}
								</button>
							{/each}
						</div>
					{/if}
					<div class="tab-panel" role="tabpanel" id="panel-{tab}" aria-labelledby="tab-{tab}">
						{@render panel(tab)}
					</div>
				{/if}
				{@render dockBottom?.()}
			</aside>
		{/if}
	</div>
	{@render after?.()}
</div>

<style>
	:global(html.pp-focus body header.header) {
		display: none;
	}
	.studio {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 520px;
		box-sizing: border-box;
		padding-block: 8px 12px;
		padding-left: max(12px, env(safe-area-inset-left, 0px));
		padding-right: max(12px, env(safe-area-inset-right, 0px));
		overflow: hidden;
	}
	.sbar {
		flex: none;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
		padding: 8px 10px;
		background: var(--pp-surface);
		border-radius: 18px;
	}
	.title {
		flex: 1 1 220px;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.title :global(.pp-eyebrow) {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.title :global(h1) {
		margin: 0;
		font-size: 21px;
		letter-spacing: -0.5px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.sbar-actions {
		flex: 0 1 auto;
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 8px;
		margin-left: auto;
	}
	.sbar-actions :global(.btn) {
		min-height: 40px;
		padding: 8px 18px;
		white-space: nowrap;
	}
	.icon-btn {
		flex: none;
		width: 40px;
		height: 40px;
		display: grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface-sunk);
		color: var(--pp-ink);
		cursor: pointer;
	}
	.icon-btn svg {
		width: 20px;
		height: 20px;
		fill: none;
		stroke: currentColor;
		stroke-width: 2.2;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.icon-btn[aria-pressed='true'] {
		background: var(--pp-ink);
		color: #fff;
	}

	.work {
		flex: 1 1 0;
		min-height: 0;
		display: flex;
		gap: 8px;
		position: relative;
	}
	.rail {
		flex: none;
		display: flex;
		flex-direction: column;
		padding: 8px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		border-radius: 18px;
		overflow-y: auto;
		scrollbar-width: none;
	}
	.center {
		flex: 1 1 0;
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.opts,
	.ctx {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px 14px;
		padding: 8px 12px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		border-radius: 18px;
	}
	.opts {
		flex-wrap: nowrap;
		overflow-x: auto;
		scrollbar-width: thin;
	}
	.opts > :global(*) {
		flex: none;
	}
	.ctx {
		flex-direction: column;
		align-items: stretch;
	}
	.ctx:not(:has(*)) {
		display: none;
	}
	.stage {
		flex: 1 1 0;
		min-height: 0;
		position: relative;
		background: var(--pp-surface-sunk);
		border-radius: 18px;
		overflow: hidden;
	}

	.dock {
		--ref-max-h: min(28vh, 260px);
		flex: none;
		width: 330px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
		overflow-y: auto;
		scrollbar-width: thin;
	}
	.dock-top {
		flex: none;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.dock-top > :global(.tile) {
		padding: 14px;
		gap: 8px;
	}
	.dock-close,
	.dock-btn,
	.scrim {
		display: none;
	}
	.tabs {
		flex: none;
		display: flex;
		gap: 4px;
		padding: 4px;
		background: var(--pp-surface-sunk);
		border-radius: 14px;
	}
	.tab {
		flex: 1 1 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: 7px 6px;
		border: 0;
		border-radius: 10px;
		background: transparent;
		font: 700 13px var(--pp-font-body);
		color: var(--pp-ink-2);
		cursor: pointer;
	}
	.tab[aria-selected='true'] {
		background: var(--pp-surface);
		color: var(--pp-ink);
		box-shadow: 0 1px 4px rgba(22, 22, 26, 0.08);
	}
	.tab-meta {
		max-width: 100%;
		font-size: 11px;
		font-weight: 600;
		color: var(--pp-ink-2);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.tab[aria-selected='true'] .tab-meta {
		color: var(--pp-accent-ink);
	}
	.tab-panel {
		padding: 14px;
		background: var(--pp-surface);
		border-radius: 20px;
	}
	.tab-panel :global(.panel) {
		width: auto;
		max-height: none;
		padding: 0;
		box-shadow: none;
	}

	/* Tablets: the dock slides in over the canvas. */
	@media (max-width: 1100px) {
		.dock {
			position: absolute;
			top: 0;
			right: 0;
			bottom: 0;
			z-index: 20;
			width: min(340px, 86vw);
			padding: 10px;
			background: var(--pp-ground);
			border-radius: 18px;
			box-shadow: -14px 0 40px rgba(22, 22, 26, 0.18);
			transform: translateX(calc(100% + 24px));
			visibility: hidden;
			transition:
				transform 0.2s ease,
				visibility 0.2s;
		}
		.dock.open {
			transform: none;
			visibility: visible;
		}
		.dock-close {
			display: grid;
			place-items: center;
			align-self: flex-end;
			flex: none;
			width: 36px;
			height: 36px;
			border: 0;
			border-radius: 10px;
			background: var(--pp-surface-sunk);
			font-size: 22px;
			cursor: pointer;
		}
		.dock-btn {
			display: grid;
		}
		.scrim {
			display: block;
			position: absolute;
			inset: 0;
			z-index: 19;
			border: 0;
			background: rgba(22, 22, 26, 0.12);
			cursor: pointer;
		}
	}

	/* Phones: tools along the bottom, the dock is a bottom sheet. */
	@media (max-width: 700px) {
		.studio {
			padding-left: max(6px, env(safe-area-inset-left, 0px));
			padding-right: max(6px, env(safe-area-inset-right, 0px));
			gap: 6px;
		}
		.sbar {
			gap: 6px;
			padding: 6px;
		}
		.title {
			flex-basis: 180px;
		}
		.title :global(.pp-eyebrow) {
			font-size: 10px;
		}
		.title :global(h1) {
			font-size: 16px;
		}
		.sbar-actions {
			flex: 1 1 100%;
			flex-wrap: wrap;
			justify-content: flex-start;
			gap: 6px;
		}
		.sbar-actions :global(.btn) {
			margin-left: auto;
			min-height: 40px;
			padding: 8px 14px;
			font-size: 15px;
		}
		.sbar-actions :global(.picker) {
			display: none;
		}
		.work {
			flex-direction: column-reverse;
			gap: 6px;
		}
		.rail {
			overflow: visible;
			padding: 6px;
		}
		.opts {
			padding: 6px 8px;
		}
		.dock {
			--ref-max-h: 26vh;
			top: auto;
			left: 0;
			width: auto;
			max-height: 72%;
			border-radius: 22px 22px 0 0;
			box-shadow: 0 -12px 34px rgba(22, 22, 26, 0.2);
			transform: translateY(calc(100% + 24px));
		}
	}
</style>
