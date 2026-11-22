import { describe, expect, it, vi } from 'vitest';
import { aiAllowed, cleanInvite, Cursors, isCursor, memberColour, Throttle } from './share';

describe('drawing together', () => {
	it('gives each member the same colour everywhere', () => {
		expect(memberColour('abc')).toBe(memberColour('abc'));
		expect(memberColour('abc')).toMatch(/^#[0-9a-f]{6}$/);
	});

	it('finds the invite code in pasted text', () => {
		expect(cleanInvite('join me:\n pp1-eyJw_b3-xyz  thanks!')).toBe('pp1-eyJw_b3-xyz');
		expect(cleanInvite('hello')).toBeNull();
	});

	it('keeps AI to solo work', () => {
		expect(aiAllowed(null)).toBe(true);
		expect(
			aiAllowed({ invite: 'pp1-x', me: { id: 'a', name: 'A' }, members: [], host: true })
		).toBe(false);
	});

	it('checks cursor messages', () => {
		expect(isCursor({ x: 1, y: 2 })).toBe(true);
		expect(isCursor({ x: 'a', y: 2 })).toBe(false);
		expect(isCursor(null)).toBe(false);
	});

	it('keeps the newest cursor of each member and drops stale ones', () => {
		const c = new Cursors(1000);
		expect(c.set('a', 1, 2, 0)).toBe(true);
		expect(c.set('a', 1, 2, 10)).toBe(false);
		expect(c.set('b', 5, 5, 500)).toBe(true);
		const list = c.list([{ id: 'a', name: 'Ana' }]);
		expect(list.map((x) => [x.id, x.name])).toEqual([
			['a', 'Ana'],
			['b', 'Someone']
		]);
		expect(c.prune(1200)).toBe(true);
		expect(c.list([]).map((x) => x.id)).toEqual(['b']);
		expect(c.remove('b')).toBe(true);
		expect(c.list([])).toEqual([]);
	});

	it('sends at most one cursor per gap, ending on the newest', () => {
		vi.useFakeTimers();
		let t = 0;
		const sent: number[] = [];
		const th = new Throttle<number>(
			(v) => sent.push(v),
			50,
			() => t
		);
		th.push(1);
		t = 10;
		th.push(2);
		t = 20;
		th.push(3);
		expect(sent).toEqual([1]);
		t = 50;
		vi.advanceTimersByTime(50);
		expect(sent).toEqual([1, 3]);
		t = 200;
		th.push(4);
		expect(sent).toEqual([1, 3, 4]);
		th.stop();
		vi.useRealTimers();
	});
});
