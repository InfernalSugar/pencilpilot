<script lang="ts">
	/**
	 * Home: start something new (templates, or any size), open a recent project or any
	 * file, and recover work left unsaved when the app last closed.
	 */
	import { resolve } from '$app/paths';
	import { goto } from '$app/navigation';
	import { isTauri } from '@tauri-apps/api/core';
	import Logo from '@pencilpilot/ui/Logo.svelte';
	import { onMount } from 'svelte';
	import { loadSettings, settings } from '#lib/settings.svelte.js';
	import {
		ago,
		askOpenPath,
		discardRecovery,
		forgetRecent,
		listRecent,
		listRecovery,
		projectName,
		SIDE_MAX,
		SIDE_MIN,
		studioQuery,
		TEMPLATES,
		type RecentItem,
		type RecoveryItem,
		type StudioStart
	} from '#lib/project.js';

	let recent = $state<RecentItem[] | null>(null);
	let recovery = $state<RecoveryItem[]>([]);
	let customOpen = $state(false);
	let custom = $state({ width: 1920, height: 1080, fps: 24 });
	const customOk = $derived(
		[custom.width, custom.height].every(
			(v) => Number.isInteger(v) && v >= SIDE_MIN && v <= SIDE_MAX
		) &&
			custom.fps >= 1 &&
			custom.fps <= 60
	);

	onMount(() => {
		if (!isTauri()) {
			recent = [];
			return;
		}
		listRecent()
			.then((r) => (recent = r))
			.catch(() => (recent = []));
		listRecovery()
			.then((r) => (recovery = r))
			.catch(() => {});
	});

	const go = (s: StudioStart) => goto(resolve(`/studio${studioQuery(s)}`));

	async function openFile() {
		await loadSettings();
		const path = await askOpenPath(settings.projectFolder);
		if (path) await go({ kind: 'open', path });
	}

	async function forget(item: RecentItem) {
		recent = (recent ?? []).filter((r) => r.path !== item.path);
		await forgetRecent(item.path).catch(() => {});
	}

	async function discard(item: RecoveryItem) {
		recovery = recovery.filter((r) => r.id !== item.id);
		await discardRecovery(item.id).catch(() => {});
	}

	/** a template's shape, scaled into a 44 px box */
	const shape = (w: number, h: number) => {
		const k = 44 / Math.max(w, h);
		return `width: ${Math.round(w * k)}px; height: ${Math.round(h * k)}px`;
	};
</script>

<main class="home">
	<header>
		<div class="brand">
			<Logo size={36} />
			<h1>PencilPilot</h1>
		</div>
		<div class="top-actions">
			<a class="ghost" href={resolve('/pen-test')}>Pen test</a>
			<a class="ghost" href={resolve('/settings')}>Settings</a>
			<button type="button" class="primary" onclick={openFile}>Open…</button>
		</div>
	</header>

	{#if recovery.length}
		<section class="recover" aria-labelledby="recover-h">
			<h2 id="recover-h">Unsaved work was found</h2>
			<p>PencilPilot closed before {recovery.length === 1 ? 'this was' : 'these were'} saved.</p>
			<ul>
				{#each recovery as item (item.id)}
					<li>
						{#if item.thumbnail}
							<img src="data:image/png;base64,{item.thumbnail}" alt="" />
						{:else}
							<span class="blank" aria-hidden="true"></span>
						{/if}
						<div class="what">
							<b>{projectName(item.path)}</b>
							<span>{item.width} × {item.height} · {ago(item.savedAt)}</span>
						</div>
						<button
							type="button"
							class="primary"
							onclick={() => go({ kind: 'recover', id: item.id, path: item.path })}
						>
							Recover
						</button>
						<button type="button" class="ghost" onclick={() => discard(item)}>Discard</button>
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	<section aria-labelledby="new-h">
		<h2 id="new-h">New</h2>
		<div class="templates">
			{#each TEMPLATES as t (t.id)}
				<button
					type="button"
					class="template"
					onclick={() => go({ kind: 'new', width: t.width, height: t.height, fps: t.fps })}
				>
					<span class="frame"><span style={shape(t.width, t.height)}></span></span>
					<b>{t.name}</b>
					<small>{t.note}</small>
				</button>
			{/each}
			<button
				type="button"
				class="template"
				aria-expanded={customOpen}
				onclick={() => (customOpen = !customOpen)}
			>
				<span class="frame"><span class="custom">+</span></span>
				<b>Custom</b>
				<small>Any size and frame rate</small>
			</button>
		</div>
		{#if customOpen}
			<form
				class="custom-form"
				onsubmit={(e) => {
					e.preventDefault();
					if (customOk) void go({ kind: 'new', ...custom });
				}}
			>
				<label>
					Width <input type="number" min={SIDE_MIN} max={SIDE_MAX} bind:value={custom.width} />
				</label>
				<label>
					Height <input type="number" min={SIDE_MIN} max={SIDE_MAX} bind:value={custom.height} />
				</label>
				<label>
					Frames a second <input type="number" min="1" max="60" bind:value={custom.fps} />
				</label>
				<button type="submit" class="primary" disabled={!customOk}>Create</button>
				{#if !customOk}
					<span class="why" role="alert">
						Sides {SIDE_MIN}–{SIDE_MAX} px, 1–60 frames a second.
					</span>
				{/if}
			</form>
		{/if}
	</section>

	<section aria-labelledby="recent-h">
		<h2 id="recent-h">Recent</h2>
		{#if recent === null}
			<p class="muted">Loading…</p>
		{:else if !recent.length}
			<p class="muted">Projects you save or open show up here.</p>
		{:else}
			<ul class="recent">
				{#each recent as item (item.path)}
					<li class:gone={!item.found}>
						<button
							type="button"
							class="card"
							disabled={!item.found}
							title={item.path}
							onclick={() => go({ kind: 'open', path: item.path })}
						>
							{#if item.thumbnail}
								<img src="data:image/png;base64,{item.thumbnail}" alt="" />
							{:else}
								<span class="blank" aria-hidden="true"></span>
							{/if}
							<b>{projectName(item.path)}</b>
							<small>
								{#if item.found}
									{item.width} × {item.height} · {item.layers} layer{item.layers === 1 ? '' : 's'} · {ago(
										item.modified
									)}
								{:else}
									Moved or deleted
								{/if}
							</small>
						</button>
						<button
							type="button"
							class="forget"
							aria-label="Remove {projectName(item.path)} from the list"
							title="Remove from the list (the file stays)"
							onclick={() => forget(item)}
						>
							×
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
</main>

<style>
	.home {
		height: 100vh;
		overflow: auto;
		box-sizing: border-box;
		padding: 24px clamp(16px, 4vw, 48px) 48px;
		-webkit-user-select: none;
		user-select: none;
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		margin-bottom: 24px;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	h1 {
		margin: 0;
		font: 800 28px var(--pp-font-display);
	}
	.top-actions {
		display: flex;
		gap: 8px;
	}
	h2 {
		margin: 28px 0 12px;
		font: 700 20px var(--pp-font-display);
	}
	.muted {
		color: var(--pp-muted);
	}
	.primary,
	.ghost {
		min-height: 44px;
		padding: 0 18px;
		display: inline-grid;
		place-items: center;
		border: 0;
		border-radius: 14px;
		font: 600 15px var(--pp-font-body);
		text-decoration: none;
		cursor: pointer;
	}
	.primary {
		background: var(--pp-accent-strong);
		color: #fff;
	}
	.primary:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.ghost {
		background: var(--pp-surface);
		color: var(--pp-ink);
	}
	.templates {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
		gap: 12px;
	}
	.template {
		display: grid;
		justify-items: start;
		gap: 4px;
		padding: 14px;
		border: 2px solid transparent;
		border-radius: 18px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		text-align: left;
		font: 14px var(--pp-font-body);
		cursor: pointer;
	}
	.template:hover,
	.template:focus-visible {
		border-color: var(--pp-accent);
		outline: none;
	}
	.template small {
		color: var(--pp-muted);
	}
	.frame {
		display: grid;
		place-items: center;
		width: 52px;
		height: 52px;
		margin-bottom: 6px;
	}
	.frame > span {
		border-radius: 4px;
		background: var(--pp-lilac);
		outline: 2px solid var(--pp-ink);
	}
	.frame .custom {
		display: grid;
		place-items: center;
		width: 40px;
		height: 40px;
		font-size: 24px;
		outline-style: dashed;
		background: none;
	}
	.custom-form {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 12px;
		margin-top: 12px;
		padding: 14px;
		border-radius: 18px;
		background: var(--pp-surface);
	}
	.custom-form label {
		display: grid;
		gap: 4px;
		font-size: 13px;
		color: var(--pp-muted);
	}
	.custom-form input {
		width: 110px;
		min-height: 40px;
		padding: 0 10px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 10px;
		font: 15px var(--pp-font-body);
	}
	.why {
		color: var(--pp-accent-ink);
		font-size: 13px;
	}
	.recent {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 14px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.recent li {
		position: relative;
	}
	.card {
		display: grid;
		gap: 4px;
		width: 100%;
		padding: 10px;
		border: 2px solid transparent;
		border-radius: 18px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		text-align: left;
		font: 14px var(--pp-font-body);
		cursor: pointer;
	}
	.card:hover:not(:disabled),
	.card:focus-visible {
		border-color: var(--pp-accent);
		outline: none;
	}
	.card:disabled {
		cursor: default;
		opacity: 0.6;
	}
	.card img,
	.card .blank {
		width: 100%;
		aspect-ratio: 16 / 10;
		object-fit: contain;
		border-radius: 12px;
		background: #fff;
		outline: 1px solid var(--pp-surface-sunk);
	}
	.card small {
		color: var(--pp-muted);
	}
	.forget {
		position: absolute;
		top: 16px;
		right: 16px;
		width: 30px;
		height: 30px;
		border: 0;
		border-radius: 50%;
		background: rgba(22, 22, 26, 0.7);
		color: #fff;
		font-size: 18px;
		cursor: pointer;
		opacity: 0;
	}
	li:hover .forget,
	.forget:focus-visible,
	.gone .forget {
		opacity: 1;
	}
	.recover {
		margin-bottom: 8px;
		padding: 16px 20px;
		border-radius: 20px;
		background: var(--pp-yellow);
	}
	.recover h2 {
		margin-top: 0;
	}
	.recover p {
		margin: -6px 0 12px;
	}
	.recover ul {
		display: grid;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.recover li {
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 8px;
		border-radius: 14px;
		background: var(--pp-surface);
	}
	.recover img,
	.recover .blank {
		width: 72px;
		height: 48px;
		object-fit: contain;
		border-radius: 8px;
		background: #fff;
	}
	.what {
		flex: 1;
		display: grid;
		min-width: 0;
	}
	.what span {
		color: var(--pp-muted);
		font-size: 13px;
	}
</style>
