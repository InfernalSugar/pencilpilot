<script lang="ts">
	import '@pencilpilot/ui/theme.css';
	import { onMount } from 'svelte';
	import { applyLook } from '#lib/appearance.js';
	import { isDark, keepSoon, loadSettings, settings } from '#lib/settings.svelte.js';

	let { children } = $props();

	/** the system's light or dark (for the "as the system" theme) */
	let systemDark = $state(false);
	/** the settings are in (until then the last theme stays) */
	let ready = $state(false);

	// the last theme, straight away (the settings file takes a moment): no white flash
	try {
		const last = localStorage.getItem('pp.theme');
		if (last === 'dark' || last === 'light') document.documentElement.dataset.theme = last;
	} catch {
		/* storage blocked */
	}

	onMount(() => {
		void loadSettings().then(() => (ready = true));
		const q = matchMedia('(prefers-color-scheme: dark)');
		systemDark = q.matches;
		const changed = () => (systemDark = q.matches);
		q.addEventListener('change', changed);
		return () => q.removeEventListener('change', changed);
	});

	// every change is kept, wherever it's made (Settings, a bar folded away…)
	$effect(() => {
		const snap = $state.snapshot(settings);
		if (!ready) return;
		void snap;
		keepSoon();
	});

	// the interface's colours follow Settings as soon as they change
	$effect(() => {
		void systemDark;
		if (!ready) return;
		const theme = isDark(settings.theme) ? 'dark' : 'light';
		document.documentElement.dataset.theme = theme;
		try {
			localStorage.setItem('pp.theme', theme);
		} catch {
			/* storage blocked */
		}
	});

	// the interface preferences (size, accent, colours, tooltips) on every screen
	$effect(() => applyLook($state.snapshot(settings)));
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
	/* Drop-downs drawn by the page, not the system: the system's own (white in some
	   desktop themes) ignored the bar's colours and left white text on white. */
	:global(select) {
		-webkit-appearance: none;
		appearance: none;
		padding-left: 10px;
		padding-right: 26px !important;
		background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 6'%3E%3Cpath d='M1 1l4 4 4-4' fill='none' stroke='%238e8d96' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E") !important;
		background-repeat: no-repeat !important;
		background-position: right 9px center !important;
		background-size: 10px 6px !important;
	}
	:global(select:not([class])) {
		background-color: var(--pp-surface);
		color: var(--pp-ink);
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 10px;
		font: 14px var(--pp-font-body);
		min-height: 34px;
		padding-left: 8px;
	}
</style>
