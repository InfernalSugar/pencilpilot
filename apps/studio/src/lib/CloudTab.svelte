<script lang="ts">
	/**
	 * Home's Cloud tab (ADR-0014): every shared project the signed-in account is in, as the
	 * cloud keeps them, to open in one click on any computer (catching up, then joining
	 * whoever is drawing). The list comes from the app's Rust side; nothing connects until
	 * this tab is shown, and never for someone signed out.
	 */
	import { resolve } from '$app/paths';
	import { onMount } from 'svelte';
	import { account, loadAccount, startSignIn } from './account.svelte';
	import { ago, type RecentItem } from './project';
	import { cleanCloud, cleanRelay, cloudList, cloudOpen, type CloudList } from './share';
	import { settings } from './settings.svelte';

	let {
		local = [],
		onrejoin,
		onopen
	}: {
		/** shared projects on this computer: opened from their file (and rejoined) */
		local?: RecentItem[];
		onrejoin: (item: RecentItem) => void;
		/** open from the cloud: a new canvas of this size, joining with this invite */
		onopen: (code: string, width: number, height: number) => void;
	} = $props();

	let list = $state<CloudList | null>(null);
	let loading = $state(false);
	let opening = $state('');
	let error = $state('');

	const net = () => ({
		relay: cleanRelay(settings.shareRelay),
		cloud: cleanCloud(settings.shareCloud)
	});

	async function load() {
		loading = true;
		error = '';
		try {
			list = await cloudList(net());
		} catch (e) {
			list = {
				status: 'unreachable',
				projects: list?.projects ?? [],
				fetched: 0,
				error: String(e)
			};
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		void (async () => {
			if (!account.loaded) await loadAccount();
			if (account.me) await load();
		})();
	});

	async function signIn() {
		await startSignIn();
		if (account.me) await load();
	}

	const iso = (secs: number) => new Date(secs * 1000).toISOString();
	/** the others in it (not this account) */
	const others = (p: CloudList['projects'][number]) =>
		p.members.filter((m) => m.user !== account.me?.user).map((m) => m.name);
	const withNames = (names: string[]) =>
		!names.length
			? 'Only you so far'
			: names.length <= 3
				? `With ${names.join(', ')}`
				: `With ${names.slice(0, 2).join(', ')} and ${names.length - 2} more`;

	async function open(p: CloudList['projects'][number]) {
		// on this computer already: its file, rejoined
		const here = local.find((r) => r.found && r.shared?.project === p.project);
		if (here) return onrejoin(here);
		opening = p.project;
		error = '';
		try {
			const o = await cloudOpen(p.project, net());
			onopen(o.code, o.width || p.width || 1920, o.height || p.height || 1080);
		} catch (e) {
			error = String(e);
		} finally {
			opening = '';
		}
	}
</script>

{#if !account.loaded}
	<p class="muted">Loading…</p>
{:else if !account.me}
	<div class="note">
		<p>
			Sign in with PencilPilot to see the shared projects you’re in, kept in the cloud, and open
			them on any computer.
		</p>
		{#if account.signingIn}
			<p class="muted" role="status">Finish signing in in your browser…</p>
		{:else}
			<button type="button" class="primary" onclick={() => void signIn()}>
				Sign in with PencilPilot
			</button>
		{/if}
		{#if account.error}<p class="why" role="alert">{account.error}</p>{/if}
	</div>
{:else if !list}
	<p class="muted" role="status">Looking in the cloud…</p>
{:else if list.status === 'off'}
	<div class="note">
		<p>No cloud is set, so shared projects are only on the computers of the people in them.</p>
		<a class="ghost" href={resolve('/settings')}>Settings</a>
	</div>
{:else}
	{#if list.status === 'unreachable'}
		<div class="note offline" role="status">
			<p>
				Can’t reach the cloud right now{list.fetched
					? `: these are the projects as of ${ago(iso(list.fetched))}.`
					: '.'}
			</p>
			<button type="button" class="ghost" disabled={loading} onclick={() => void load()}>
				{loading ? 'Trying…' : 'Try again'}
			</button>
		</div>
	{/if}
	{#if error}<p class="why" role="alert">{error}</p>{/if}
	{#if !list.projects.length}
		{#if list.status === 'ok'}
			<p class="muted">
				Projects you draw together show up here once the cloud keeps them: host or join a session to
				start one.
			</p>
		{/if}
	{:else}
		<ul class="recent">
			{#each list.projects as p (p.project)}
				<li>
					<button
						type="button"
						class="card"
						disabled={!!opening}
						aria-label="Open {p.name}"
						onclick={() => void open(p)}
					>
						{#if p.thumb}
							<img src="data:image/png;base64,{p.thumb}" alt="" />
						{:else}
							<span class="blank" aria-hidden="true"></span>
						{/if}
						<b>{p.name}</b>
						<small>{withNames(others(p))} · {ago(iso(p.changed))}</small>
						{#if p.online.length}
							<small class="online">
								<i aria-hidden="true"></i>
								{p.online.join(', ')}
								{p.online.length === 1 ? 'is' : 'are'} drawing now
							</small>
						{/if}
						{#if p.full}
							<small class="full">Cloud space full: newer changes aren’t kept</small>
						{/if}
						{#if opening === p.project}<small role="status">Opening…</small>{/if}
					</button>
				</li>
			{/each}
		</ul>
	{/if}
{/if}

<style>
	.muted {
		color: var(--pp-muted);
	}
	.note {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 12px 16px;
		padding: 14px 18px;
		border-radius: 18px;
		background: var(--pp-surface);
	}
	.note p {
		flex: 1 1 280px;
		margin: 0;
	}
	.note.offline {
		margin-bottom: 14px;
	}
	.why {
		color: var(--pp-accent-ink);
		font-size: 13px;
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
	.ghost {
		background: var(--pp-ground);
		color: var(--pp-ink);
	}
	.ghost:disabled {
		opacity: 0.6;
		cursor: default;
	}
	.recent {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 14px;
		margin: 0;
		padding: 0;
		list-style: none;
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
		cursor: progress;
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
	.online {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.online i {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: #2a9d8f;
	}
	.card .full {
		color: var(--pp-accent-ink);
	}
</style>
