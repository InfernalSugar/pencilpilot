<script lang="ts">
	/**
	 * Customise workspaces: the one in use, its name, the bars and panels it opens with,
	 * the side panel's tab and the tools on its strip (every change shows straight away),
	 * plus new, delete and reset.
	 */
	import { settings } from './settings.svelte';
	import {
		BUILT_IN,
		MAX_WORKSPACES,
		PANEL_TABS,
		STRIP_TOOLS,
		cleanTools,
		copyOf,
		freshId,
		freshName,
		isBuiltIn
	} from './workspaces';
	import type { StudioLayout } from '@pencilpilot/paint/Studio.svelte';

	let {
		tab = $bindable(),
		onuse,
		onclose
	}: {
		/** the side panel's tab in use */
		tab: string;
		/** switch to a workspace (the studio rearranges) */
		onuse: (id: string) => void;
		onclose: () => void;
	} = $props();

	const ws = $derived(
		settings.workspaces.find((w) => w.id === settings.workspace) ?? settings.workspaces[0]
	);

	const PARTS: { key: keyof StudioLayout; name: string }[] = [
		{ key: 'rail', name: 'Tool strip' },
		{ key: 'options', name: 'Options bar' },
		{ key: 'context', name: 'Tool options' },
		{ key: 'bottom', name: 'Timeline' },
		{ key: 'dock', name: 'Side panel' }
	];

	function toggleTool(id: string, on: boolean) {
		const now = (ws.tools as string[]).filter((t) => t !== id);
		ws.tools = cleanTools(on ? [...now, id] : now, ws.tools);
	}
	function newWorkspace() {
		if (settings.workspaces.length >= MAX_WORKSPACES) return;
		const w = copyOf($state.snapshot(ws));
		w.id = freshId(settings.workspaces);
		w.name = freshName(settings.workspaces, `${ws.name} copy`);
		settings.workspaces.push(w);
		onuse(w.id);
	}
	function remove() {
		if (settings.workspaces.length <= 1) return;
		const i = settings.workspaces.findIndex((w) => w.id === ws.id);
		const next = settings.workspaces[i + 1] ?? settings.workspaces[i - 1];
		onuse(next.id);
		settings.workspaces.splice(i, 1);
	}
	function reset() {
		const d = BUILT_IN.find((w) => w.id === ws.id);
		if (!d) return;
		const i = settings.workspaces.findIndex((w) => w.id === ws.id);
		settings.workspaces[i] = copyOf(d);
		onuse(d.id);
	}

	let dialog = $state<HTMLDialogElement>();
	$effect(() => {
		if (dialog && !dialog.open) dialog.showModal();
	});
</script>

<dialog
	bind:this={dialog}
	class="workspaces"
	aria-label="Customise workspaces"
	oncancel={(e) => {
		e.preventDefault();
		onclose();
	}}
>
	<h2>Workspaces</h2>
	<p class="now">
		Each workspace is a layout of the same canvas: the panels it opens with and the tools on its
		strip. Changes show straight away and stay with the workspace.
	</p>
	<div class="top">
		<label>
			Workspace
			<select value={ws.id} onchange={(e) => onuse(e.currentTarget.value)}>
				{#each settings.workspaces as w (w.id)}
					<option value={w.id}>{w.name}</option>
				{/each}
			</select>
		</label>
		<label>
			Name
			<input
				type="text"
				maxlength="40"
				value={ws.name}
				onchange={(e) => {
					const v = e.currentTarget.value.trim();
					if (v) ws.name = v;
					else e.currentTarget.value = ws.name;
				}}
			/>
		</label>
	</div>

	<fieldset>
		<legend>Shows</legend>
		<div class="checks">
			{#each PARTS as p (p.key)}
				<label class="check">
					<input
						type="checkbox"
						checked={!!settings.layout[p.key]}
						onchange={(e) =>
							(settings.layout = { ...settings.layout, [p.key]: e.currentTarget.checked })}
					/>
					{p.name}
				</label>
			{/each}
		</div>
		<label class="inline">
			Side panel opens on
			<select bind:value={tab}>
				{#each PANEL_TABS as t (t.id)}
					<option value={t.id}>{t.name}</option>
				{/each}
			</select>
		</label>
	</fieldset>

	<fieldset>
		<legend>Tools on the strip</legend>
		<div class="checks tools" aria-label="Tools on the strip">
			{#each STRIP_TOOLS as t (t.id)}
				<label class="check">
					<input
						type="checkbox"
						checked={ws.tools.includes(t.id)}
						disabled={t.id === 'brush'}
						onchange={(e) => toggleTool(t.id, e.currentTarget.checked)}
					/>
					{t.name}
				</label>
			{/each}
		</div>
		<p class="hint">A tool's key works in every workspace, shown or not.</p>
	</fieldset>

	<div class="actions">
		<button
			type="button"
			disabled={settings.workspaces.length >= MAX_WORKSPACES}
			onclick={newWorkspace}
		>
			New workspace
		</button>
		{#if isBuiltIn(ws.id)}
			<button type="button" onclick={reset}>Reset “{ws.name}”</button>
		{/if}
		<button type="button" disabled={settings.workspaces.length <= 1} onclick={remove}>
			Delete “{ws.name}”
		</button>
		<span class="gap"></span>
		<button type="button" class="go" onclick={onclose}>Done</button>
	</div>
</dialog>

<style>
	.workspaces {
		width: min(620px, calc(100vw - 32px));
		max-height: calc(100dvh - 32px);
		padding: 20px;
		border: 0;
		border-radius: 20px;
		background: var(--pp-paper, #fffdf6);
		color: var(--pp-ink, #16161a);
		font: 14px var(--pp-font-body);
		box-shadow: 0 24px 60px rgba(0, 0, 0, 0.3);
	}
	.workspaces::backdrop {
		background: rgba(22, 22, 26, 0.4);
	}
	h2 {
		margin: 0 0 4px;
		font: 700 20px var(--pp-font-display);
	}
	.now,
	.hint {
		margin: 0 0 14px;
		color: var(--pp-muted);
		font-size: 13px;
	}
	.hint {
		margin: 8px 0 0;
	}
	.top {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
		margin-bottom: 14px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 12px;
		font-weight: 600;
	}
	label.inline {
		flex-direction: row;
		align-items: center;
		gap: 8px;
		margin-top: 10px;
	}
	select,
	input[type='text'] {
		box-sizing: border-box;
		height: 34px;
		padding: 0 8px;
		border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
		border-radius: 10px;
		background: var(--pp-surface);
		color: inherit;
		font: inherit;
		font-weight: 400;
	}
	fieldset {
		margin: 0 0 12px;
		padding: 12px;
		border: 1px solid color-mix(in srgb, currentColor 12%, transparent);
		border-radius: 14px;
	}
	legend {
		padding: 0 4px;
		font-weight: 700;
	}
	.checks {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
		gap: 6px 12px;
	}
	label.check {
		flex-direction: row;
		align-items: center;
		gap: 8px;
		font-size: 13px;
		font-weight: 500;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: 16px;
	}
	.gap {
		flex: 1;
	}
	.actions button {
		height: 38px;
		padding: 0 16px;
		border: 0;
		border-radius: 12px;
		background: var(--pp-track);
		color: inherit;
		font: 600 14px var(--pp-font-body);
		cursor: pointer;
	}
	.actions .go {
		background: var(--pp-accent-strong, #e8492f);
		color: #fff;
	}
	.actions button:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	@media (max-width: 560px) {
		.top {
			grid-template-columns: 1fr;
		}
	}
</style>
