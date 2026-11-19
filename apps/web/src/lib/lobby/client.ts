/**
 * Browser side of a lobby: gets a ticket, keeps a WebSocket open (reconnecting
 * after drops) and hands server messages to the page.
 */
import type { ClientMsg, LobbyView, ServerMsg } from './protocol';

export interface Joined {
	ticket: string;
	playerId: string;
	name: string;
}

const guestKey = (code: string) => `pp.lobby.${code}`;

/** Guests keep one random id per lobby in this tab, so a reload rejoins as the same player. */
export function guestId(code: string) {
	try {
		let id = sessionStorage.getItem(guestKey(code));
		if (!id) {
			id = Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) =>
				'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-'.charAt(b % 64)
			).join('');
			sessionStorage.setItem(guestKey(code), id);
		}
		return id;
	} catch {
		return 'g' + Math.random().toString(36).slice(2, 14).padEnd(12, 'x');
	}
}

export async function getTicket(code: string, name?: string): Promise<Joined> {
	const res = await fetch(`/api/lobby/${code}/ticket`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ name, playerId: guestId(code) })
	});
	if (!res.ok)
		throw new Error((await res.json().catch(() => null))?.message ?? `HTTP ${res.status}`);
	return res.json();
}

export class LobbyClient {
	#ws: WebSocket | null = null;
	#closed = false;
	#retry = 0;
	/** server clock minus ours, ms */
	offset = 0;

	constructor(
		readonly code: string,
		readonly ticket: string,
		readonly onmessage: (m: ServerMsg) => void,
		readonly onstatus: (s: 'connecting' | 'open' | 'lost') => void
	) {
		this.#open();
	}

	#open() {
		this.onstatus(this.#retry ? 'lost' : 'connecting');
		const proto = location.protocol === 'https:' ? 'wss' : 'ws';
		const ws = new WebSocket(`${proto}://${location.host}/lobby-ws`);
		this.#ws = ws;
		ws.onopen = () => {
			this.#retry = 0;
			ws.send(JSON.stringify({ t: 'join', code: this.code, ticket: this.ticket }));
			this.onstatus('open');
		};
		ws.onmessage = (e) => {
			const m = JSON.parse(e.data) as ServerMsg;
			if (m.t === 'state') this.offset = m.lobby.serverNow - Date.now();
			if (m.t === 'kicked') this.#closed = true;
			this.onmessage(m);
		};
		ws.onclose = () => {
			if (this.#closed) return;
			this.onstatus('lost');
			const wait = Math.min(8000, 500 * 2 ** this.#retry++);
			setTimeout(() => !this.#closed && this.#open(), wait);
		};
	}

	send(m: ClientMsg) {
		if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(JSON.stringify(m));
	}

	/** milliseconds left until a server time */
	left(serverTime: number) {
		return serverTime - (Date.now() + this.offset);
	}

	close() {
		this.#closed = true;
		this.#ws?.close();
	}
}

export const isHost = (l: LobbyView, you: string) => l.players.some((p) => p.id === you && p.host);
