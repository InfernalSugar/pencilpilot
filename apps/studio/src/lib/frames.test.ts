import { describe, expect, it } from 'vitest';
import { KEEP_MOST, memoryPlan } from './frames';

const GB = 1 << 30;
const MB = 1 << 20;

describe('the memory limit for frames', () => {
	it('is shared: three quarters for the store, the rest for the engine’s recent drawings', () => {
		const p = memoryPlan(16 * GB, 1920, 1080);
		expect(p.store).toBe(12 * GB);
		// a 1080p drawing is ~8 MB: plenty of room, so the most
		expect(p.keep).toBe(KEEP_MOST);
		expect(p.store + p.engine).toBeLessThanOrEqual(16 * GB);
	});

	it('is never passed, even for big canvases', () => {
		for (const [limit, w, h] of [
			[1 * GB, 1920, 1080],
			[1 * GB, 3840, 2160],
			[512 * MB, 3840, 2160],
			[2 * GB, 8192, 8192]
		]) {
			const p = memoryPlan(limit, w, h);
			expect(p.store + p.engine, `${limit} ${w}×${h}`).toBeLessThanOrEqual(limit);
		}
		// 4K at 1 GB: a quarter is 256 MB, a drawing ~32 MB → 8
		expect(memoryPlan(GB, 3840, 2160).keep).toBe(8);
	});

	it('keeps no spare drawings when not even one fits (they come from the store)', () => {
		expect(memoryPlan(256 * MB, 8192, 8192).keep).toBe(0);
	});
});
