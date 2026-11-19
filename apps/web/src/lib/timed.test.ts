import { beforeEach, describe, expect, it } from 'vitest';
import {
	addToSession,
	clockOf,
	currentSession,
	sessionDrawing,
	startSession,
	timeLabel
} from './timed';
import type { Submission } from './submission';

const sub = (at: number) => ({ at, png: '', width: 1, height: 1 }) as unknown as Submission;

describe('timed drawing', () => {
	it('labels and clocks', () => {
		expect(timeLabel(30)).toBe('30-second');
		expect(timeLabel(120)).toBe('2-minute');
		expect(clockOf(125)).toBe('2:05');
		expect(clockOf(0.2)).toBe('0:01');
		expect(clockOf(-3)).toBe('0:00');
	});

	describe('gesture sessions', () => {
		beforeEach(() => {
			startSession({ count: 3, time: 30, category: 'animals', topic: null, tier: 1 });
		});
		it('counts drawn and skipped pictures', () => {
			expect(addToSession(sub(1))).toBe(1);
			expect(addToSession(null)).toBe(2);
			expect(addToSession(sub(3))).toBe(3);
			expect(currentSession()?.drawings.map((d) => d.at)).toEqual([1, 3]);
			expect(currentSession()?.skipped).toBe(1);
			expect(sessionDrawing(3)?.at).toBe(3);
			expect(sessionDrawing(2)).toBeNull();
		});
	});
});
