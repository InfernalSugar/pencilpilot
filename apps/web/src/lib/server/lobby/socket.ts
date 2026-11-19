/**
 * The lobby's WebSocket endpoint (/lobby-ws). Attached to the dev, preview
 * and production HTTP servers; everything else goes through the hub.
 */
import type { Server } from 'node:http';
import { WebSocketServer, type WebSocket } from 'ws';
import type { ClientMsg, ServerMsg } from '../../lobby/protocol';
import { getHub } from './hub';
import { readTicket } from './ticket';

export const LOBBY_PATH = '/lobby-ws';
/** Messages per second a connection may send (strokes stream at ~30/s). */
const RATE = 120;

export function attachLobbySockets(server: Server) {
	const g = server as Server & { __ppLobby?: boolean };
	if (g.__ppLobby) return;
	g.__ppLobby = true;
	const wss = new WebSocketServer({ noServer: true, maxPayload: 3_000_000 });
	server.on('upgrade', (req, socket, head) => {
		// other upgrades (e.g. Vite's hot reload) aren't ours
		if (!req.url || new URL(req.url, 'http://x').pathname !== LOBBY_PATH) return;
		wss.handleUpgrade(req, socket, head, (ws) => connection(ws));
	});
}

function connection(ws: WebSocket) {
	const hub = getHub();
	let joined: { code: string; playerId: string } | null = null;
	const send = (m: ServerMsg) => {
		if (ws.readyState === ws.OPEN) ws.send(JSON.stringify(m));
	};
	let tokens = RATE;
	const refill = setInterval(() => (tokens = RATE), 1000);
	const ping = setInterval(() => ws.readyState === ws.OPEN && ws.ping(), 25_000);

	ws.on('message', (data) => {
		if (--tokens < 0) return;
		let msg: ClientMsg;
		try {
			msg = JSON.parse(String(data));
		} catch {
			return;
		}
		if (!msg || typeof msg !== 'object' || typeof msg.t !== 'string') return;
		if (!joined) {
			if (msg.t !== 'join') return;
			const code = String(msg.code ?? '').toUpperCase();
			const who = readTicket(code, msg.ticket);
			if (!who) {
				send({ t: 'error', message: 'Please reload the page to join again.' });
				return ws.close();
			}
			const err = hub.join(code, who, send);
			if (err) {
				send({ t: 'error', message: err });
				return ws.close();
			}
			joined = { code, playerId: who.playerId };
			return;
		}
		hub.handle(joined.code, joined.playerId, msg);
	});
	ws.on('close', () => {
		clearInterval(refill);
		clearInterval(ping);
		if (joined) hub.disconnect(joined.code, joined.playerId, send);
	});
}
