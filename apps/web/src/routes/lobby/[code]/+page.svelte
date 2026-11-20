<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import Tile from '@pencilpilot/ui/Tile.svelte';
	import Button from '@pencilpilot/ui/Button.svelte';
	import { getTicket, LobbyClient } from '#lib/lobby/client.js';
	import WaitingRoom from '#lib/lobby/WaitingRoom.svelte';
	import FreeDraw from '#lib/lobby/FreeDraw.svelte';
	import RoundDraw from '#lib/lobby/RoundDraw.svelte';
	import Results from '#lib/lobby/Results.svelte';
	import type { ClientMsg, LobbyView, ServerMsg } from '#lib/lobby/protocol.js';

	const code = page.params.code!.toUpperCase();
	let client: LobbyClient | null = null;
	let lobby = $state<LobbyView | null>(null);
	let you = $state('');
	let conn = $state<'connecting' | 'open' | 'lost'>('connecting');
	let fatal = $state('');
	let toast = $state('');
	let toastTimer: ReturnType<typeof setTimeout> | undefined;
	let askName = $state(false);
	let name = $state('');
	let joining = $state(false);
	let free: ReturnType<typeof FreeDraw> | undefined = $state();
	let round: ReturnType<typeof RoundDraw> | undefined = $state();

	const send = (m: ClientMsg) => client?.send(m);
	const left = (t: number) => client?.left(t) ?? 0;

	function onMessage(m: ServerMsg) {
		switch (m.t) {
			case 'state':
				lobby = m.lobby;
				you = m.you;
				return;
			case 'time-up':
				return void round?.submitNow();
			case 'kicked':
				fatal = 'The host removed you from this lobby.';
				client?.close();
				return;
			case 'error':
				if (!lobby) {
					fatal = m.message;
					client?.close();
				} else {
					toast = m.message;
					clearTimeout(toastTimer);
					toastTimer = setTimeout(() => (toast = ''), 4000);
				}
				return;
			default:
				free?.onMessage(m);
		}
	}

	async function join(guestName?: string) {
		joining = true;
		try {
			const t = await getTicket(code, guestName);
			try {
				localStorage.setItem('pp.lobbyName', t.name);
			} catch {
				/* storage blocked */
			}
			askName = false;
			client = new LobbyClient(code, t.ticket, onMessage, (s) => (conn = s));
		} catch (e) {
			fatal = e instanceof Error ? e.message : String(e);
		} finally {
			joining = false;
		}
	}

	onMount(() => {
		if (page.data.user) void join();
		else {
			try {
				name = localStorage.getItem('pp.lobbyName') ?? '';
			} catch {
				/* storage blocked */
			}
			askName = true;
		}
		return () => client?.close();
	});
</script>

<svelte:head><title>Lobby {code} · PencilPilot</title></svelte:head>

{#if lobby && lobby.phase === 'free'}
	<FreeDraw bind:this={free} {lobby} {you} {send} reconnecting={conn !== 'open'} />
{:else if lobby && (lobby.phase === 'drawing' || lobby.phase === 'collecting')}
	<RoundDraw bind:this={round} {lobby} {you} {send} {left} reconnecting={conn !== 'open'} />
{:else}
	<div class="pp-page">
		{#if fatal}
			<Tile tone="lilac">
				<h1>Can’t join this lobby</h1>
				<p>{fatal}</p>
				<div><Button href="/lobby">Back to lobbies</Button></div>
			</Tile>
		{:else if askName}
			<Tile as="section" label="Join">
				<span class="pp-eyebrow">Lobby {code}</span>
				<h1>Join the lobby</h1>
				<form
					class="name"
					onsubmit={(e) => {
						e.preventDefault();
						void join(name);
					}}
				>
					<label>
						<span>Your name (others will see it)</span>
						<input bind:value={name} maxlength="24" required autocomplete="nickname" />
					</label>
					<Button type="submit" disabled={joining || !name.trim()}>
						{joining ? 'Joining…' : 'Join'}
					</Button>
				</form>
				<p class="small">
					Or <a href="/login?next=/lobby/{code}">sign in</a> to play with your account.
				</p>
			</Tile>
		{:else if !lobby}
			<p class="status" role="status">Joining lobby {code}…</p>
		{:else}
			<div class="head">
				<span class="pp-eyebrow">
					Lobby {lobby.code} · {lobby.settings.mode === 'free' ? 'Free draw' : 'Competition'}
				</span>
				{#if conn !== 'open'}<span class="conn" role="status">Reconnecting…</span>{/if}
			</div>
			{#if lobby.phase === 'waiting'}
				<h1>Lobby</h1>
				<WaitingRoom {lobby} {you} {send} />
			{:else}
				<Results {lobby} {you} {send} />
			{/if}
		{/if}
		{#if toast}<p class="toast" role="alert">{toast}</p>{/if}
	</div>
{/if}
{#if toast && lobby && ['free', 'drawing', 'collecting'].includes(lobby.phase)}
	<p class="toast" role="alert">{toast}</p>
{/if}

<style>
	h1 {
		font-size: 40px;
		letter-spacing: -1px;
		margin-bottom: 12px;
	}
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}
	.conn {
		padding: 6px 12px;
		border-radius: 999px;
		background: var(--pp-yellow);
		font-weight: 600;
		font-size: 13px;
	}
	.status {
		font-weight: 600;
		color: var(--pp-muted);
	}
	.name {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 10px;
	}
	.name label {
		display: flex;
		flex-direction: column;
		gap: 4px;
		font-size: 13px;
		font-weight: 600;
	}
	.name input {
		min-height: 48px;
		width: min(280px, 100%);
		padding: 8px 14px;
		border: 2px solid var(--pp-track);
		border-radius: 14px;
		background: var(--pp-surface);
		color: var(--pp-ink);
		font: 600 17px var(--pp-font-body);
	}
	.small {
		margin: 0;
		font-size: 14px;
		color: var(--pp-ink-2);
	}
	.toast {
		position: fixed;
		left: 50%;
		bottom: 24px;
		transform: translateX(-50%);
		z-index: 50;
		margin: 0;
		padding: 12px 20px;
		border-radius: 999px;
		background: var(--pp-ink);
		color: #fff;
		font-weight: 600;
		max-width: calc(100vw - 32px);
	}
</style>
