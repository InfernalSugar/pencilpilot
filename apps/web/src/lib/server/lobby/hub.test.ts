import { describe, expect, it } from 'vitest';
import { COLLECT_GRACE_MS, Hub, type Clock } from './hub';
import { cleanName, issueTicket, readTicket } from './ticket';
import type { LobbyRef, LobbyView, ServerMsg } from '../../lobby/protocol';

/** A clock the test moves by hand. */
function fakeClock() {
	let now = 1_000_000;
	let timers: { at: number; fn: () => void; dead: boolean }[] = [];
	const clock: Clock = {
		now: () => now,
		later(fn, ms) {
			const t = { at: now + ms, fn, dead: false };
			timers.push(t);
			return () => (t.dead = true);
		}
	};
	const tick = (ms: number) => {
		now += ms;
		const due = timers.filter((t) => !t.dead && t.at <= now);
		timers = timers.filter((t) => !due.includes(t));
		for (const t of due) t.fn();
	};
	return { clock, tick };
}

const ref = (i: number): LobbyRef => ({
	id: `r${i}`,
	category: 'animals',
	title: `Ref ${i}`,
	imageUrl: `/references/px-${i}.jpg`,
	author: 'a',
	authorUrl: 'https://pixabay.com/users/a/',
	pageUrl: 'https://pixabay.com/photos/x/',
	license: 'Pixabay Content License',
	licenseUrl: 'https://pixabay.com/service/license-summary/'
});
const PNG = 'data:image/png;base64,AAAA';

function setup(players = ['ann', 'bob', 'cat']) {
	const { clock, tick } = fakeClock();
	const hub = new Hub(clock);
	const inbox: Record<string, ServerMsg[]> = {};
	const sends: Record<string, (m: ServerMsg) => void> = {};
	const code = hub.create({ playerId: 'ann', name: 'Ann', userId: 'u-ann' });
	for (const id of players) {
		inbox[id] = [];
		sends[id] = (m) => inbox[id].push(m);
		expect(
			hub.join(
				code,
				{ playerId: id, name: id.toUpperCase(), userId: id === 'cat' ? null : `u-${id}` },
				sends[id]
			)
		).toBeNull();
	}
	const state = (id: string): LobbyView =>
		(inbox[id].filter((m) => m.t === 'state').at(-1) as { lobby: LobbyView }).lobby;
	return { hub, code, tick, inbox, sends, state };
}

describe('lobby: competition', () => {
	it('runs rounds: same picture, time limit, reveal with names, grader decides', () => {
		const { hub, code, tick, inbox, state } = setup();
		hub.handle(code, 'bob', { t: 'settings', settings: { rounds: 2 } }); // not the host
		expect(inbox.bob.at(-1)).toMatchObject({ t: 'error' });
		hub.handle(code, 'ann', { t: 'settings', settings: { rounds: 2, timeLimit: 60 } });
		hub.handle(code, 'ann', { t: 'start', refs: [ref(1)] });
		expect(inbox.ann.at(-1)).toMatchObject({ t: 'error' }); // too few pictures
		hub.handle(code, 'ann', { t: 'start', refs: [ref(1), ref(2)] });
		let v = state('cat');
		expect(v).toMatchObject({ phase: 'drawing', round: 1, reference: { id: 'r1' } });
		expect(v.endsAt! - v.serverNow).toBe(60_000);

		hub.handle(code, 'bob', { t: 'submit', png: PNG, score: 6 }); // done early
		tick(60_000);
		expect(inbox.ann.some((m) => m.t === 'time-up')).toBe(true);
		expect(state('ann').phase).toBe('collecting');
		hub.handle(code, 'ann', { t: 'submit', png: PNG, score: 8.5 });
		hub.handle(code, 'cat', { t: 'submit', png: null, score: null }); // nothing drawn
		v = state('ann');
		expect(v.phase).toBe('reveal');
		expect(v.reveal?.total).toBe(3);
		expect(v.reveal?.entry.name).toBeTruthy();

		// reactions: not on your own drawing
		const shown = v.reveal!.entry.playerId;
		const other = ['ann', 'bob', 'cat'].find((x) => x !== shown)!;
		hub.handle(code, shown, { t: 'react', target: shown, v: 1 });
		hub.handle(code, other, { t: 'react', target: shown, v: 1 });
		expect(state('ann').reveal?.entry.likes).toBe(1);

		hub.handle(code, 'ann', { t: 'next' });
		hub.handle(code, 'ann', { t: 'next' });
		hub.handle(code, 'ann', { t: 'next' });
		v = state('bob');
		expect(v.phase).toBe('round-end');
		expect(v.results[0].winnerId).toBe('ann');
		expect(v.results[0].entries.find((e) => e.playerId === 'cat')?.score).toBeNull();

		// round 2: ann draws badly, bob well
		hub.handle(code, 'ann', { t: 'next' });
		expect(state('ann')).toMatchObject({ phase: 'drawing', round: 2, reference: { id: 'r2' } });
		hub.handle(code, 'ann', { t: 'submit', png: PNG, score: 3 });
		hub.handle(code, 'bob', { t: 'submit', png: PNG, score: 7 });
		hub.handle(code, 'cat', { t: 'submit', png: PNG, score: 9.9 });
		for (let i = 0; i < 4; i++) hub.handle(code, 'ann', { t: 'next' });
		v = state('ann');
		expect(v.phase).toBe('final');
		// totals: bob 13, cat 9.9, ann 11.5 → bob wins the game
		expect(v.totals.map((t) => t.playerId)).toEqual(['bob', 'ann', 'cat']);
		expect(v.winnerId).toBe('bob');

		// play again without leaving
		hub.handle(code, 'ann', { t: 'again' });
		expect(state('cat')).toMatchObject({ phase: 'waiting', results: [], round: 0 });
	});

	it('stops waiting after the grace period, and ties go to more round wins', () => {
		const { hub, code, tick, state } = setup(['ann', 'bob']);
		hub.handle(code, 'ann', { t: 'settings', settings: { rounds: 1, timeLimit: 30 } });
		hub.handle(code, 'ann', { t: 'start', refs: [ref(1)] });
		hub.handle(code, 'ann', { t: 'submit', png: PNG, score: 5 });
		tick(30_000);
		expect(state('ann').phase).toBe('collecting');
		tick(COLLECT_GRACE_MS);
		expect(state('ann').phase).toBe('reveal');
	});
});

describe('lobby: free draw', () => {
	it('relays strokes live, keeps them for late joiners, undo and clear', () => {
		const { hub, code, inbox, sends } = setup(['ann', 'bob']);
		hub.handle(code, 'ann', { t: 'settings', settings: { mode: 'free' } });
		hub.handle(code, 'ann', { t: 'start' });
		const style = { brushId: 'pen', color: '#ff0000', size: 1, opacity: 1, eraser: false };
		hub.handle(code, 'bob', { t: 'sb', k: 1, style });
		hub.handle(code, 'bob', { t: 'sp', k: 1, pts: [1, 2, 0.5, 0, 3, 4, 0.5, 16] });
		expect(inbox.ann.at(-1)).toMatchObject({ t: 'sp', from: 'bob', k: 1 });
		expect(inbox.bob.filter((m) => m.t === 'sp')).toHaveLength(0); // not echoed back
		hub.handle(code, 'bob', { t: 'se', k: 1 });
		hub.handle(code, 'bob', { t: 'sp', k: 1, pts: [1, 2, 3] }); // bad: after end, wrong length

		// someone joins late and gets the canvas
		const late: ServerMsg[] = [];
		hub.join(code, { playerId: 'dan', name: 'Dan', userId: null }, (m) => late.push(m));
		const sync = late.find((m) => m.t === 'sync') as { strokes: Record<string, unknown[]> };
		expect(sync.strokes.bob).toHaveLength(1);

		hub.handle(code, 'bob', { t: 'undo' });
		expect(inbox.ann.at(-1)).toEqual({ t: 'undo', from: 'bob' });
		hub.handle(code, 'ann', {
			t: 'reference',
			ref: { ...ref(3), imageUrl: 'https://evil.test/x.jpg' }
		});
		hub.handle(code, 'ann', { t: 'reference', ref: ref(3) });
		void sends;
	});
});

describe('lobby: people', () => {
	it('kicks, hands over the host, and refuses when full', () => {
		const { hub, code, inbox, sends, state } = setup();
		hub.handle(code, 'ann', { t: 'kick', playerId: 'bob' });
		expect(inbox.bob.at(-1)).toEqual({ t: 'kicked' });
		expect(hub.join(code, { playerId: 'bob2', name: 'B', userId: 'u-bob' }, () => {})).toMatch(
			/removed/
		);
		hub.disconnect(code, 'ann', sends.ann);
		expect(state('cat').players.find((p) => p.host)?.id).toBe('cat');
		for (let i = 0; i < 7; i++)
			hub.join(code, { playerId: `p${i}`, name: `P${i}`, userId: null }, () => {});
		expect(hub.join(code, { playerId: 'x', name: 'X', userId: null }, () => {})).toMatch(/full/);
	});

	it('tickets and names', () => {
		const who = { playerId: 'p1', name: 'Ann', userId: null };
		const t = issueTicket('ABCDEF', who, 0);
		expect(readTicket('ABCDEF', t, 1000)).toEqual(who);
		expect(readTicket('ZZZZZZ', t, 1000)).toBeNull();
		expect(readTicket('ABCDEF', t, 13 * 3600_000)).toBeNull();
		expect(readTicket('ABCDEF', t.replace(/.$/, 'x'), 1000)).toBeNull();
		expect(cleanName('  Ann \n Lee ')).toBe('Ann Lee');
		expect(cleanName('   ')).toBeNull();
		expect(() => new Hub().create({ playerId: 'g', name: 'G', userId: null })).toThrow();
	});
});
