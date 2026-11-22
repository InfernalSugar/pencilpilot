<script lang="ts">
	/** Settings: keyboard shortcuts, pen, touch and files. Changes are kept as you make them. */
	import { resolve } from '$app/paths';
	import { isTauri } from '@tauri-apps/api/core';
	import PenFeel from '@pencilpilot/paint/PenFeel.svelte';
	import { onMount } from 'svelte';
	import {
		AUTOSAVE_CHOICES,
		loadSettings,
		resetSettings,
		saveSettings,
		settings
	} from '#lib/settings.svelte.js';
	import { cleanRelay } from '#lib/share.js';
	import {
		ACTIONS,
		clashes,
		comboOf,
		DEFAULT_KEYS,
		showKey,
		type ActionId
	} from '#lib/shortcuts.js';

	let ready = $state(false);
	let dataFolder = $state('');
	let note = $state('');
	/** the action waiting for its new key */
	let capturing = $state<ActionId | null>(null);
	/** the last change that took another action's key (it got this one's old key) */
	let swapped = $state('');
	const mac = typeof navigator !== 'undefined' && /Mac|iPad/.test(navigator.platform);
	const label = (id: ActionId) => ACTIONS.find((a) => a.id === id)?.label ?? id;

	onMount(() => {
		loadSettings().then(() => (ready = true));
		if (isTauri())
			import('@tauri-apps/api/core').then(({ invoke }) =>
				invoke<string>('data_folder')
					.then((f) => (dataFolder = f))
					.catch(() => {})
			);
	});

	// keep every change (once loaded)
	$effect(() => {
		const snap = $state.snapshot(settings);
		if (!ready) return;
		void snap;
		saveSettings().catch((e) => (note = `Couldn’t keep the settings: ${e}`));
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

	async function chooseFolder() {
		const { open } = await import('@tauri-apps/plugin-dialog');
		const f = await open({
			directory: true,
			multiple: false,
			defaultPath: settings.projectFolder ?? undefined
		});
		if (typeof f === 'string') settings.projectFolder = f;
	}

	const groups = [...new Set(ACTIONS.map((a) => a.group))];
</script>

<svelte:window onkeydowncapture={onkey} />

<main class="settings">
	<header>
		<a class="back" href={resolve('/')} aria-label="Back to home">←</a>
		<h1>Settings</h1>
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
	</header>
	{#if note}<p class="note" role="alert">{note}</p>{/if}

	<section aria-labelledby="keys-h">
		<h2 id="keys-h">Keyboard shortcuts</h2>
		<p class="hint">
			Click Change, then press the new key (Esc to stop). Taking another action’s key swaps them.
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
	</section>

	<section aria-labelledby="pen-h">
		<h2 id="pen-h">Pen</h2>
		<label class="row">
			<input type="checkbox" bind:checked={settings.penPressure} />
			<span>
				Use pen pressure and tilt <small>(off: every stroke at a steady medium pressure)</small>
			</span>
		</label>
		<div class="feel"><PenFeel /></div>
	</section>

	<section aria-labelledby="touch-h">
		<h2 id="touch-h">Touch</h2>
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
	</section>

	<section aria-labelledby="files-h">
		<h2 id="files-h">Files</h2>
		<div class="row">
			<span>Projects folder: <b>{settings.projectFolder ?? 'the system’s choice'}</b></span>
			<button type="button" class="ghost small" onclick={chooseFolder}>Choose…</button>
			{#if settings.projectFolder}
				<button type="button" class="ghost small" onclick={() => (settings.projectFolder = null)}>
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
			<p class="hint">Recovery copies, versions and settings live in <code>{dataFolder}</code>.</p>
		{/if}
	</section>

	<section aria-labelledby="together-h">
		<h2 id="together-h">Drawing together</h2>
		<label class="row">
			<span>Your name</span>
			<input maxlength="24" placeholder="What others see" bind:value={settings.shareName} />
		</label>
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
			The relay introduces the apps of people drawing together and carries their drawing when they
			can’t connect directly. It’s encrypted; the relay can’t see what you draw. Leave it empty for
			the default, or give your own (see <code>deploy/relay</code>). Solo projects never connect to
			anything.
		</p>
	</section>
</main>

<style>
	.settings {
		height: 100vh;
		overflow: auto;
		box-sizing: border-box;
		max-width: 860px;
		margin: 0 auto;
		padding: 24px 24px 64px;
	}
	header {
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
		margin: 32px 0 8px;
		font: 700 20px var(--pp-font-display);
	}
	h3 {
		margin: 16px 0 6px;
		font-size: 14px;
		color: var(--pp-muted);
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	section {
		padding: 4px 0;
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
	.back {
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
		color: var(--pp-accent-ink);
	}
	.swapped {
		padding: 8px 12px;
		border-radius: 10px;
		background: var(--pp-yellow);
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
		padding: 6px 10px;
		border-radius: 12px;
		background: var(--pp-surface);
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
	.row input[type='number'] {
		width: 80px;
		min-height: 36px;
		padding: 0 8px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 10px;
	}
	select {
		min-height: 36px;
		border-radius: 10px;
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
</style>
