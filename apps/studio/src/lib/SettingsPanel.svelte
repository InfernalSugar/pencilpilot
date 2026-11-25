<script lang="ts" module>
	export type SettingsSection =
		| 'look'
		| 'pen'
		| 'keys'
		| 'files'
		| 'together'
		| 'cursor'
		| 'brushes'
		| 'canvas'
		| 'selection'
		| 'guides'
		| 'performance'
		| 'exports'
		| 'prompts';
	/** the section last open (it opens there again) */
	let lastSection: SettingsSection = 'look';
</script>

<script lang="ts">
	/**
	 * Settings, in sections: look, pen and touch, keyboard shortcuts, files, drawing
	 * together. Changes apply and are kept as you make them. The studio shows it as a
	 * window over the canvas (`onclose`); home's Settings page shows it full size.
	 */
	import { resolve } from '$app/paths';
	import { isTauri } from '@tauri-apps/api/core';
	import PenFeel from '@pencilpilot/paint/PenFeel.svelte';
	import { onMount, tick } from 'svelte';
	import {
		AUTOSAVE_CHOICES,
		DEFAULT_LAYOUT,
		resetSettings,
		saving,
		settings,
		type Theme
	} from '#lib/settings.svelte.js';
	import { cleanCloud, cleanRelay } from '#lib/share.js';
	import {
		account,
		endAccount,
		loadAccount,
		startSignIn,
		stopSignIn
	} from '#lib/account.svelte.js';
	import Interface from '#lib/settings/Interface.svelte';
	import { PREFERENCE_GROUPS } from '#lib/settings/groups.js';
	import {
		ACTIONS,
		clashes,
		comboOf,
		DEFAULT_KEYS,
		showKey,
		type ActionId
	} from '#lib/shortcuts.js';

	let {
		onclose
	}: {
		/** shown as a window: close it (none: a page of its own, with a way home) */
		onclose?: () => void;
	} = $props();

	const SECTIONS: { id: SettingsSection; label: string }[] = [
		{ id: 'look', label: 'Appearance' },
		{ id: 'pen', label: 'Pen and touch' },
		{ id: 'keys', label: 'Keyboard shortcuts' },
		// the preferences (each group its own component); Interface is part of Appearance
		...PREFERENCE_GROUPS.filter((g) => g.id !== 'interface').map((g) => ({
			id: g.id as SettingsSection,
			label: g.title
		})),
		{ id: 'files', label: 'Files' },
		{ id: 'together', label: 'Drawing together' }
	];
	const THEMES: [Theme, string][] = [
		['light', 'Light'],
		['dark', 'Dark'],
		['system', 'As the system is set']
	];

	let section = $state<SettingsSection>(lastSection);
	$effect(() => {
		lastSection = section;
	});
	let dataFolder = $state('');
	/** the action waiting for its new key */
	let capturing = $state<ActionId | null>(null);
	/** the last change that took another action's key (it got this one's old key) */
	let swapped = $state('');
	const mac = typeof navigator !== 'undefined' && /Mac|iPad/.test(navigator.platform);
	const label = (id: ActionId) => ACTIONS.find((a) => a.id === id)?.label ?? id;
	const groups = [...new Set(ACTIONS.map((a) => a.group))];

	let dialog = $state<HTMLDialogElement>();
	/** the last press began outside the window */
	let downOutside = false;
	let tabs = $state<HTMLElement>();

	onMount(() => {
		if (isTauri()) void loadAccount();
		if (isTauri())
			import('@tauri-apps/api/core').then(({ invoke }) =>
				invoke<string>('data_folder')
					.then((f) => (dataFolder = f))
					.catch(() => {})
			);
		if (dialog && !dialog.open) {
			dialog.showModal();
			tabs?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus();
		}
	});

	function onkey(e: KeyboardEvent) {
		if (!capturing) return;
		e.preventDefault();
		e.stopPropagation();
		if (e.key === 'Escape') {
			capturing = null;
			return;
		}
		const combo = comboOf(e);
		if (!combo) return; // a modifier on its own: wait for the key
		const id = capturing;
		const old = settings.keys[id];
		// a key another action has: they swap
		for (const other of clashes(settings.keys, id, combo)) {
			settings.keys[other] = old;
			swapped = `${label(other)} now uses ${showKey(old, mac)}.`;
		}
		settings.keys[id] = combo;
		capturing = null;
	}

	/** up and down (or left and right) move between the sections */
	async function tabKey(e: KeyboardEvent) {
		const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
		const at = SECTIONS.findIndex((s) => s.id === section);
		let to = step ? (at + step + SECTIONS.length) % SECTIONS.length : -1;
		if (e.key === 'Home') to = 0;
		if (e.key === 'End') to = SECTIONS.length - 1;
		if (to < 0) return;
		e.preventDefault();
		section = SECTIONS[to].id;
		await tick();
		tabs?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus();
	}

	async function chooseFolder() {
		const { open } = await import('@tauri-apps/plugin-dialog');
		const f = await open({
			directory: true,
			multiple: false,
			defaultPath: settings.projectFolder ?? undefined
		});
		if (typeof f === 'string') settings.projectFolder = f;
	}
</script>

<svelte:window onkeydowncapture={onkey} />

{#snippet body()}
	<header>
		{#if !onclose}
			<a class="back" href={resolve('/')} aria-label="Back to home">←</a>
		{/if}
		<h1 id="settings-h">Settings</h1>
		<button
			type="button"
			class="ghost"
			onclick={() => {
				resetSettings();
				swapped = '';
			}}
		>
			Reset all
		</button>
		{#if onclose}
			<button type="button" class="ghost close" aria-label="Close the settings" onclick={onclose}>
				×
			</button>
		{/if}
	</header>
	{#if saving.error}<p class="note" role="alert">{saving.error}</p>{/if}

	<div class="split">
		<div
			class="sections"
			role="tablist"
			aria-label="Settings sections"
			aria-orientation="vertical"
			bind:this={tabs}
		>
			{#each SECTIONS as s (s.id)}
				<button
					type="button"
					role="tab"
					id="settings-tab-{s.id}"
					aria-selected={section === s.id}
					aria-controls="settings-panel"
					tabindex={section === s.id ? 0 : -1}
					onclick={() => (section = s.id)}
					onkeydown={tabKey}
				>
					{s.label}
				</button>
			{/each}
		</div>

		<div
			class="panel"
			id="settings-panel"
			role="tabpanel"
			aria-labelledby="settings-tab-{section}"
			tabindex="-1"
		>
			{#if section === 'look'}
				<h2>Appearance</h2>
				<fieldset>
					<legend>Interface</legend>
					{#each THEMES as [id, text] (id)}
						<label class="row">
							<input type="radio" name="theme" value={id} bind:group={settings.theme} />
							<span>{text}</span>
						</label>
					{/each}
				</fieldset>
				<p class="hint">Only the panels and bars change: the paper stays white.</p>

				<h3>Bars and panels</h3>
				<p class="hint">
					Fold a bar away with its arrow button, and drag the edge of the timeline or the side panel
					to make it bigger or smaller.
				</p>
				<label class="row">
					<input type="checkbox" bind:checked={settings.layout.options} />
					<span>Brush options bar</span>
				</label>
				<label class="row">
					<input type="checkbox" bind:checked={settings.layout.context} />
					<span>Tool options and guides bar</span>
				</label>
				<label class="row">
					<input type="checkbox" bind:checked={settings.layout.bottom} />
					<span>Timeline</span>
				</label>
				<label class="row">
					<input type="checkbox" bind:checked={settings.layout.dock} />
					<span>Side panel (layers, brushes, colour…)</span>
				</label>
				<div class="row">
					<button
						type="button"
						class="ghost small"
						onclick={() => (settings.layout = structuredClone(DEFAULT_LAYOUT))}
					>
						Show everything at the usual size
					</button>
				</div>

				<h3>Size, colours and hands</h3>
				<Interface />
			{:else if section === 'pen'}
				<h2>Pen and touch</h2>
				<label class="row">
					<input type="checkbox" bind:checked={settings.penPressure} />
					<span>
						Use pen pressure and tilt <small>(off: every stroke at a steady medium pressure)</small>
					</span>
				</label>
				<div class="feel"><PenFeel /></div>
				<fieldset>
					<legend>One finger</legend>
					{#each [['auto', 'Draws until you use a pen, then moves the view'], ['draw', 'Always draws'], ['move', 'Always moves the view (pen only for drawing)']] as [id, text] (id)}
						<label class="row">
							<input type="radio" name="fingers" value={id} bind:group={settings.fingers} />
							<span>{text}</span>
						</label>
					{/each}
				</fieldset>
				<label class="row">
					<input type="checkbox" bind:checked={settings.tapGestures} />
					<span>Two-finger tap undoes, three-finger tap redoes</span>
				</label>
			{:else if section === 'keys'}
				<h2>Keyboard shortcuts</h2>
				<p class="hint">
					Click Change, then press the new key (Esc to stop). Taking another action’s key swaps
					them.
				</p>
				{#if swapped}<p class="swapped" role="status">{swapped}</p>{/if}
				{#each groups as g (g)}
					<h3>{g}</h3>
					<ul class="keys">
						{#each ACTIONS.filter((a) => a.group === g) as a (a.id)}
							<li data-action={a.id}>
								<span class="what">{a.label}</span>
								{#if capturing === a.id}
									<kbd class="waiting" role="status">Press a key…</kbd>
								{:else}
									<kbd>{showKey(settings.keys[a.id], mac)}</kbd>
								{/if}
								<button
									type="button"
									class="ghost small"
									aria-label="Change the key for {a.label}"
									onclick={() => {
										swapped = '';
										capturing = capturing === a.id ? null : a.id;
									}}
								>
									{capturing === a.id ? 'Cancel' : 'Change'}
								</button>
								{#if settings.keys[a.id] !== DEFAULT_KEYS[a.id]}
									<button
										type="button"
										class="ghost small"
										aria-label="Reset the key for {a.label}"
										onclick={() => (settings.keys[a.id] = DEFAULT_KEYS[a.id])}
									>
										Reset
									</button>
								{/if}
							</li>
						{/each}
					</ul>
				{/each}
			{:else if section === 'files'}
				<h2>Files</h2>
				<div class="row">
					<span>Projects folder: <b>{settings.projectFolder ?? 'the system’s choice'}</b></span>
					<button type="button" class="ghost small" onclick={chooseFolder}>Choose…</button>
					{#if settings.projectFolder}
						<button
							type="button"
							class="ghost small"
							onclick={() => (settings.projectFolder = null)}
						>
							Clear
						</button>
					{/if}
				</div>
				<label class="row">
					<span>Keep a recovery copy of unsaved work</span>
					<select bind:value={settings.autosaveSeconds}>
						{#each AUTOSAVE_CHOICES as s (s)}
							<option value={s}>
								{s === 0
									? 'Never'
									: s < 60
										? `Every ${s} seconds`
										: `Every ${s / 60} minute${s === 60 ? '' : 's'}`}
							</option>
						{/each}
					</select>
				</label>
				<label class="row">
					<span>Earlier versions kept per project</span>
					<input type="number" min="0" max="100" bind:value={settings.versionsKept} />
				</label>
				{#if dataFolder}
					<p class="hint">
						Recovery copies, versions and settings live in <code>{dataFolder}</code>.
					</p>
				{/if}
			{:else if section === 'together'}
				<h2>Drawing together</h2>
				<div class="row">
					<span>PencilPilot account</span>
					{#if !isTauri()}
						<span class="hint">In the app</span>
					{:else if account.me}
						<span>
							Signed in as <b>{account.me.name}</b>
							<button type="button" class="ghost small" onclick={() => void endAccount()}>
								Sign out
							</button>
						</span>
					{:else if account.signingIn}
						<span>
							Finish signing in in your browser…
							<button type="button" class="ghost small" onclick={() => void stopSignIn()}>
								Cancel
							</button>
						</span>
					{:else}
						<button type="button" class="ghost small" onclick={() => void startSignIn()}>
							Sign in with PencilPilot
						</button>
					{/if}
				</div>
				{#if account.error}<p class="hint" role="alert">{account.error}</p>{/if}
				<p class="hint">
					Drawing together needs a PencilPilot account: others see the name on it. You can unlink
					this app on the website’s Account page.
				</p>
				<label class="row">
					<span>Relay</span>
					<input
						placeholder="Default (iroh’s public relays)"
						aria-invalid={!!settings.shareRelay.trim() && !cleanRelay(settings.shareRelay)}
						bind:value={settings.shareRelay}
					/>
				</label>
				{#if settings.shareRelay.trim() && !cleanRelay(settings.shareRelay)}
					<p class="hint">That isn’t an https address: the default relays are used.</p>
				{/if}
				<p class="hint">
					The relay introduces the apps of people drawing together and carries their drawing when
					they can’t connect directly. It’s encrypted; the relay can’t see what you draw. Leave it
					empty for the default, or give your own (see <code>deploy/relay</code>). Solo projects
					never connect to anything.
				</p>
				<label class="row">
					<span>Cloud</span>
					<input
						placeholder="Default"
						spellcheck="false"
						aria-invalid={!!settings.shareCloud.trim() && !cleanCloud(settings.shareCloud)}
						bind:value={settings.shareCloud}
					/>
				</label>
				{#if settings.shareCloud.trim() && !cleanCloud(settings.shareCloud)}
					<p class="hint">
						That isn’t a cloud address: it’s the cloud’s id, then <code>@host:port</code>.
					</p>
				{/if}
				<p class="hint">
					The cloud keeps a copy of your shared projects, so you can carry on when no one else is
					online and open them on any computer (home’s Cloud tab). Only the projects’ members can
					get them. Leave it empty for the default, type <code>off</code> for none, or give your own
					(see <code>deploy/keeper</code>).
				</p>
			{:else}
				{@const g = PREFERENCE_GROUPS.find((x) => x.id === section)}
				{#if g}
					<h2>{g.title}</h2>
					<g.body />
				{/if}
			{/if}
		</div>
	</div>
{/snippet}

{#if onclose}
	<dialog
		bind:this={dialog}
		class="settings window"
		aria-labelledby="settings-h"
		oncancel={(e) => {
			e.preventDefault();
			// Esc while a key is being chosen only stops that
			if (!capturing) onclose();
		}}
		onpointerdown={(e) => (downOutside = e.target === dialog)}
		onclick={(e) => {
			// a click on the dimmed canvas around the window closes it (not a drag out of it)
			if (e.target === dialog && downOutside) onclose();
		}}
	>
		<div class="inner">{@render body()}</div>
	</dialog>
{:else}
	<main class="settings page">
		<div class="inner">{@render body()}</div>
	</main>
{/if}

<style>
	.page {
		height: 100vh;
		overflow: hidden;
		box-sizing: border-box;
		padding: 24px;
	}
	.page .inner {
		height: 100%;
		max-width: 1000px;
		margin: 0 auto;
	}
	.window {
		width: min(960px, calc(100vw - 48px));
		height: min(720px, calc(100vh - 48px));
		max-width: none;
		max-height: none;
		padding: 0;
		border: 0;
		border-radius: 24px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.35);
	}
	/* (the padding is inside: a click on the window itself is a click outside it) */
	.window .inner {
		padding: 20px 24px;
	}
	.window::backdrop {
		background: rgba(22, 22, 26, 0.35);
	}
	.inner {
		display: flex;
		flex-direction: column;
		gap: 12px;
		height: 100%;
		min-height: 0;
		font: 14px var(--pp-font-body);
	}
	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 14px;
	}
	h1 {
		flex: 1;
		margin: 0;
		font: 800 28px var(--pp-font-display);
	}
	h2 {
		margin: 0 0 12px;
		font: 700 22px var(--pp-font-display);
	}
	h3 {
		margin: 20px 0 6px;
		font-size: 13px;
		color: var(--pp-muted);
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	.split {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: 210px minmax(0, 1fr);
		gap: 16px;
	}
	.sections {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.sections button {
		padding: 11px 14px;
		border: 0;
		border-radius: 12px;
		background: none;
		color: var(--pp-ink-2);
		font: 600 14px var(--pp-font-body);
		text-align: left;
		cursor: pointer;
	}
	.sections button:hover {
		background: var(--pp-surface-sunk);
	}
	.sections button[aria-selected='true'] {
		background: var(--pp-surface);
		color: var(--pp-ink);
		box-shadow: inset 3px 0 0 var(--pp-accent);
	}
	.panel {
		min-height: 0;
		overflow-y: auto;
		padding: 18px 20px 28px;
		border-radius: 18px;
		background: var(--pp-surface);
		scrollbar-width: thin;
	}
	.panel:focus {
		outline: none;
	}
	.back,
	.ghost {
		min-height: 40px;
		padding: 0 14px;
		display: inline-grid;
		place-items: center;
		border: 0;
		border-radius: 12px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 14px var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
	}
	.panel .ghost {
		background: var(--pp-surface-sunk);
	}
	.back,
	.close {
		width: 40px;
		padding: 0;
		font-size: 20px;
	}
	.small {
		min-height: 34px;
		padding: 0 12px;
		font-size: 13px;
	}
	.hint,
	small {
		color: var(--pp-muted);
		font-size: 13px;
	}
	.note {
		margin: 0;
		color: var(--pp-accent-ink);
	}
	.swapped {
		padding: 8px 12px;
		border-radius: 10px;
		background: var(--pp-yellow);
		color: #16161a;
		font-size: 14px;
	}
	.keys {
		display: grid;
		gap: 4px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.keys li {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 4px 6px 4px 12px;
		border-radius: 12px;
		background: var(--pp-ground);
	}
	.what {
		flex: 1;
	}
	kbd {
		min-width: 44px;
		padding: 4px 10px;
		border-radius: 8px;
		background: var(--pp-ink);
		color: var(--pp-on-ink);
		font: 600 13px var(--pp-font-body);
		text-align: center;
	}
	.waiting {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.row {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 10px;
		margin: 10px 0;
	}
	.row input[type='number'],
	.row input:not([type]) {
		min-height: 36px;
		padding: 0 10px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 10px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font: 14px var(--pp-font-body);
	}
	.row input[type='number'] {
		width: 80px;
	}
	input[type='radio'],
	input[type='checkbox'] {
		accent-color: var(--pp-accent-strong);
	}
	fieldset {
		margin: 0;
		padding: 0;
		border: 0;
	}
	legend {
		font-weight: 600;
	}
	.feel {
		max-width: 420px;
	}
	code {
		font-size: 12px;
	}

	/* narrow windows: the sections go across the top */
	@media (max-width: 700px) {
		.split {
			grid-template-columns: minmax(0, 1fr);
			grid-template-rows: auto minmax(0, 1fr);
		}
		.sections {
			flex-direction: row;
			overflow-x: auto;
		}
		.sections button {
			flex: none;
		}
	}
</style>
