import { describe, expect, it } from 'vitest';
import { wav } from './audio';
import { addCut, cutStart, fromFile, newTimeline, toFile, toggleMarker } from './timeline';

describe('sound', () => {
	it('writes a mix as a 16-bit stereo WAV', () => {
		const left = new Float32Array([0, 1, -1, 0.5]);
		const buf = {
			length: 4,
			sampleRate: 48000,
			numberOfChannels: 1,
			getChannelData: () => left
		} as unknown as AudioBuffer;
		const w = wav(buf);
		const v = new DataView(w.buffer);
		expect(new TextDecoder().decode(w.subarray(0, 4))).toBe('RIFF');
		expect(new TextDecoder().decode(w.subarray(8, 12))).toBe('WAVE');
		expect(v.getUint16(22, true)).toBe(2); // stereo (mono is doubled)
		expect(v.getUint32(24, true)).toBe(48000);
		expect(w.length).toBe(44 + 4 * 4);
		// frame 1: full scale both sides; frame 2: full negative
		expect([v.getInt16(48, true), v.getInt16(50, true)]).toEqual([32767, 32767]);
		expect(v.getInt16(52, true)).toBe(-32768);
	});

	it('places cuts along their scene, with markers kept in the file', () => {
		const tl = newTimeline([{ id: 1, drawing: 1 }]);
		const s = tl.scenes[0];
		s.cuts[0].tracks[1] = [{ drawing: 1, length: 10 }];
		addCut(s, 0);
		s.cuts[1].tracks[1] = [{ drawing: 2, length: 4 }];
		expect(cutStart(s, 0)).toBe(0);
		expect(cutStart(s, 1)).toBe(10);
		expect(toggleMarker(s, 12, 'beat')).toBe(true);
		expect(toggleMarker(s, 3)).toBe(true);
		expect(s.markers!.map((m) => m.frame)).toEqual([3, 12]);
		expect(toggleMarker(s, 3)).toBe(false);
		s.audio = [{ id: 1, name: 'music', file: 'k.mp3', start: -5, gain: 0.7, muted: true }];
		const back = fromFile(toFile(tl, [1]), new Map([[1, 1]]));
		expect(back.scenes[0].markers).toEqual([{ frame: 12, label: 'beat' }]);
		expect(back.scenes[0].audio).toEqual([
			{ id: 1, name: 'music', file: 'k.mp3', start: -5, gain: 0.7, muted: true }
		]);
		toggleMarker(s, 12);
		expect(s.markers).toBeUndefined();
	});
});
