<script lang="ts">
	/**
	 * Home: start something new (templates, or any size), draw together (host, join, or
	 * go back to a shared project), open a recent project or any file, and recover work
	 * left unsaved when the app last closed.
	 */
	import { resolve } from '$app/paths';
	import { goto } from '$app/navigation';
	import { isTauri } from '@tauri-apps/api/core';
	import Logo from '@pencilpilot/ui/Logo.svelte';
	import { onMount } from 'svelte';
	import { loadSettings, settings } from '#lib/settings.svelte.js';
	import { fromUnits, showLength, toUnits } from '@pencilpilot/paint/view-prefs';
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
	import { cleanInvite } from '#lib/share.js';
	import { askTogether } from '#lib/together-start.js';
	import CloudTab from '#lib/CloudTab.svelte';

	let recent = $state<RecentItem[] | null>(null);
	/** projects drawn together get their own row; the others stay in Recent */
	const shared = $derived((recent ?? []).filter((r) => r.shared));
	const solo = $derived(recent && recent.filter((r) => !r.shared));
	/** Draw together is the desktop app's (it connects peer to peer) */
	const together = isTauri();
	let invite = $state('');
	const inviteCode = $derived(cleanInvite(invite));
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

	/** a side in the units Settings names (2 decimals; pixels whole) */
	const side = (px: number) => +toUnits(px, settings.units, settings.ppi).toFixed(2);
	const setSide = (k: 'width' | 'height', v: number) =>
		(custom[k] = fromUnits(v, settings.units, settings.ppi));

	const sideRange = $derived(
		settings.units === 'px'
			? `${SIDE_MIN}–${SIDE_MAX} px`
			: `${showLength(SIDE_MIN, settings.units, settings.ppi)}–${showLength(SIDE_MAX, settings.units, settings.ppi)}`
	);

	onMount(() => {
		// the size and frame rate Settings gives new projects
		void loadSettings().then(() => {
			custom = { width: settings.newWidth, height: settings.newHeight, fps: settings.newFps };
		});
		// the first time home shows after the app starts: the last project, if Settings says
		let first = false;
		try {
			first = !sessionStorage.getItem('pp.started');
			sessionStorage.setItem('pp.started', '1');
		} catch {
			/* storage blocked */
		}
		if (!isTauri()) {
			recent = [];
			return;
		}
		listRecent()
			.then(async (r) => {
				recent = r;
				await loadSettings();
				if (first && settings.reopenLast && r[0]?.found) void go({ kind: 'open', path: r[0].path });
			})
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

	/** a new canvas (HD), then Draw together hosts or joins */
	function host() {
		askTogether({ what: 'host' });
		void go({ kind: 'new', width: 1920, height: 1080, fps: 24 });
	}
	function join() {
		if (!inviteCode) return;
		askTogether({ what: 'join', code: inviteCode });
		void go({ kind: 'new', width: 1920, height: 1080, fps: 24 });
	}
	function rejoin(item: RecentItem) {
		askTogether({ what: 'rejoin' });
		void go({ kind: 'open', path: item.path });
	}
	/** from the Cloud tab, a project not on this computer: a new canvas, then join it */
	function openFromCloud(code: string, width: number, height: number) {
		askTogether({ what: 'cloud', code });
		void go({ kind: 'new', width, height, fps: 24 });
	}

	/** the projects' tabs: this computer's recent ones, or the cloud's shared ones */
	const showRecent = $derived(settings.recentCount > 0);
	let tab = $state<'recent' | 'cloud'>('recent');
	function pick(t: 'recent' | 'cloud') {
		tab = t;
		try {
			localStorage.setItem('pp.home.tab', t);
		} catch {
			/* storage blocked */
		}
	}
	try {
		if (localStorage.getItem('pp.home.tab') === 'cloud') tab = 'cloud';
	} catch {
		/* storage blocked */
	}
	const names = (w: string[]) =>
		!w.length
			? 'Shared'
			: w.length <= 3
				? `With ${w.join(', ')}`
				: `With ${w.slice(0, 2).join(', ')} and ${w.length - 2} more`;

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
				{#each [['width', 'Width'], ['height', 'Height']] as const as [k, label] (k)}
					<label>
						{label} ({settings.units})
						<input
							type="number"
							min={side(SIDE_MIN)}
							max={side(SIDE_MAX)}
							step={settings.units === 'px' ? 1 : 0.01}
							value={side(custom[k])}
							oninput={(e) => setSide(k, +e.currentTarget.value)}
						/>
					</label>
				{/each}
				<label>
					Frames a second <input type="number" min="1" max="60" bind:value={custom.fps} />
				</label>
				<button type="submit" class="primary" disabled={!customOk}>Create</button>
				{#if settings.units !== 'px'}
					<span class="px" data-testid="custom-px">
						{custom.width} × {custom.height} px at {settings.ppi} ppi
					</span>
				{/if}
				{#if !customOk}
					<span class="why" role="alert">
						Sides {sideRange}, 1–60 frames a second.
					</span>
				{/if}
			</form>
		{/if}
	</section>

	{#if together}
		<section aria-labelledby="together-h">
			<h2 id="together-h">Draw together</h2>
			<ul class="recent">
				<li>
					<button type="button" class="card start" onclick={host}>
						<span class="blank big" aria-hidden="true">
							<svg viewBox="0 0 24 24">
								<circle cx="9" cy="8" r="3.2" /><path d="M3.5 19a5.5 5.5 0 0 1 11 0" /><path
									d="M17 8v6M14 11h6"
								/>
							</svg>
						</span>
						<b>Host a session</b>
						<small>A new canvas, and a code to invite others</small>
					</button>
				</li>
				<li>
					<form
						class="card start join"
						onsubmit={(e) => {
							e.preventDefault();
							join();
						}}
					>
						<label for="invite"><b>Join with a code</b></label>
						<small>Paste the code someone sent you</small>
						<input
							id="invite"
							placeholder="pp1-…"
							autocomplete="off"
							spellcheck="false"
							aria-invalid={!!invite.trim() && !inviteCode}
							bind:value={invite}
						/>
						<button type="submit" class="primary" disabled={!inviteCode}>Join</button>
					</form>
				</li>
				{#each shared as item (item.path)}
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
								{names(item.shared?.with ?? [])}{item.found ? ` · ${ago(item.modified)}` : ''}
							</small>
						</button>
						{#if item.found}
							<button
								type="button"
								class="primary rejoin"
								aria-label="Rejoin {projectName(item.path)}"
								onclick={() => rejoin(item)}
							>
								Rejoin
							</button>
						{/if}
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
		</section>
	{/if}

	{#if showRecent && together}
		<div class="tabs" role="tablist" aria-label="Projects">
			{#each [['recent', 'Recent'], ['cloud', 'Cloud']] as const as [id, label] (id)}
				<button
					type="button"
					role="tab"
					id="{id}-h"
					aria-selected={tab === id}
					aria-controls="{id}-panel"
					tabindex={tab === id ? 0 : -1}
					onclick={() => pick(id)}
					onkeydown={(e) => {
						if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
							pick(tab === 'recent' ? 'cloud' : 'recent');
							document.getElementById(`${tab}-h`)?.focus();
						}
					}}
				>
					{label}
				</button>
			{/each}
		</div>
	{/if}

	{#if together && (tab === 'cloud' || !showRecent)}
		<section id="cloud-panel" aria-labelledby="cloud-h">
			{#if !showRecent}<h2 id="cloud-h">Cloud</h2>{/if}
			<CloudTab local={shared} onrejoin={rejoin} onopen={openFromCloud} />
		</section>
	{:else if showRecent}
		<section id="recent-panel" aria-labelledby="recent-h">
			{#if !together}<h2 id="recent-h">Recent</h2>{/if}
			{#if solo === null}
				<p class="muted">Loading…</p>
			{:else if !solo.length}
				<p class="muted">Projects you save or open show up here.</p>
			{:else}
				<ul class="recent">
					{#each solo.slice(0, settings.recentCount) as item (item.path)}
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
										{item.width} × {item.height} · {item.layers} layer{item.layers === 1 ? '' : 's'} ·
										{ago(item.modified)}
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
	{/if}
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
	/* the projects' tabs read as the sections' headings */
	.tabs {
		display: flex;
		gap: 6px;
		margin: 28px 0 12px;
	}
	.tabs [role='tab'] {
		padding: 4px 2px 6px;
		margin-right: 14px;
		border: 0;
		border-bottom: 3px solid transparent;
		background: none;
		color: var(--pp-muted);
		font: 700 20px var(--pp-font-display);
		cursor: pointer;
	}
	.tabs [role='tab'][aria-selected='true'] {
		color: var(--pp-ink);
		border-bottom-color: var(--pp-accent);
	}
	.tabs [role='tab']:focus-visible {
		outline: 2px solid var(--pp-accent);
		outline-offset: 2px;
		border-radius: 6px;
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
	.px {
		align-self: center;
		color: var(--pp-muted);
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
	.start {
		align-content: start;
		min-height: 100%;
	}
	.blank.big {
		display: grid;
		place-items: center;
		color: var(--pp-accent);
		background: var(--pp-lilac);
		outline: 0;
	}
	.blank.big svg {
		width: 56px;
		height: 56px;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	.join {
		grid-template-columns: minmax(0, 1fr);
		cursor: default;
		gap: 6px;
		padding: 14px;
	}
	.join input {
		width: 100%;
		min-width: 0;
		min-height: 44px;
		margin-top: auto;
		padding: 0 12px;
		border: 2px solid var(--pp-surface-sunk);
		border-radius: 12px;
		background: var(--pp-ground);
		color: var(--pp-ink);
		font: 15px var(--pp-font-body);
	}
	.join input[aria-invalid='true'] {
		border-color: var(--pp-accent);
	}
	li:has(.rejoin) .card b,
	li:has(.rejoin) .card small {
		padding-right: 84px;
	}
	.rejoin {
		position: absolute;
		right: 16px;
		bottom: 14px;
		min-height: 36px;
		padding: 0 14px;
		font-size: 14px;
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
		/* yellow in both themes: dark words on it */
		color: #16161a;
	}
	.recover li {
		color: var(--pp-ink);
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
