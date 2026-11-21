/**
 * Sound for the animation: the files a project's scenes play, decoded once, with
 * waveforms for the timeline, playback in step with the frames, a short snippet while
 * scrubbing (to find a word or a beat by ear), and a mixdown for video exports.
 *
 * Clips are placed by scene frame (`AudioClip.start`); a scene's cuts play one after
 * another, so music can run under several cuts.
 */
import type { AudioClip } from './timeline';

export interface Sound {
	key: string;
	name: string;
	/** the file as imported (kept in the project as it is) */
	bytes: Uint8Array;
	buffer: AudioBuffer | null;
}

/** File types the studio takes (what browsers decode). */
export const AUDIO_TYPES = '.wav,.mp3,.ogg,.oga,.opus,.m4a,.aac,.flac,.webm,audio/*';

export class AudioBank {
	#ctx: AudioContext | null = null;
	#sounds = new Map<string, Sound>();
	#playing: { src: AudioBufferSourceNode; gain: GainNode }[] = [];
	#peaks = new Map<string, Float32Array>();
	#next = 1;

	/** the audio context (made on first use: browsers want a click before sound) */
	ctx(): AudioContext {
		this.#ctx ??= new AudioContext({ latencyHint: 'interactive' });
		if (this.#ctx.state === 'suspended') void this.#ctx.resume();
		return this.#ctx;
	}

	get(key: string) {
		return this.#sounds.get(key);
	}
	keys() {
		return [...this.#sounds.keys()];
	}

	/** A new sound file (decoded); its key. Throws if it isn't sound the studio can play. */
	async add(bytes: Uint8Array, name: string): Promise<string> {
		const ext = (name.match(/\.([a-z0-9]{1,5})$/i)?.[1] ?? 'bin').toLowerCase();
		const key = `s${this.#next++}-${Date.now().toString(36)}.${ext}`;
		await this.put(key, bytes, name);
		return key;
	}

	/** A sound under a known key (a project being opened). */
	async put(key: string, bytes: Uint8Array, name = key) {
		const buffer = await this.ctx().decodeAudioData(bytes.slice().buffer);
		this.#sounds.set(key, { key, name, bytes, buffer });
		for (const k of [...this.#peaks.keys()]) if (k.startsWith(`${key}@`)) this.#peaks.delete(k);
	}

	/** Forget every sound (a new document). */
	clear() {
		this.stop();
		this.#sounds.clear();
		this.#peaks.clear();
	}

	/** How long a sound is, in frames at `fps`. */
	frames(key: string, fps: number): number {
		const b = this.#sounds.get(key)?.buffer;
		return b ? Math.ceil(b.duration * fps) : 0;
	}

	/** The waveform: the loudest point of each frame (0–1), at `fps`. */
	peaks(key: string, fps: number): Float32Array {
		const id = `${key}@${fps}`;
		const have = this.#peaks.get(id);
		if (have) return have;
		const b = this.#sounds.get(key)?.buffer;
		if (!b) return new Float32Array(0);
		const per = b.sampleRate / fps;
		const n = Math.ceil(b.length / per);
		const out = new Float32Array(n);
		for (let c = 0; c < b.numberOfChannels; c++) {
			const d = b.getChannelData(c);
			for (let f = 0; f < n; f++) {
				const end = Math.min(d.length, Math.floor((f + 1) * per));
				let m = out[f];
				for (let i = Math.floor(f * per); i < end; i++) {
					const v = d[i] < 0 ? -d[i] : d[i];
					if (v > m) m = v;
				}
				out[f] = m;
			}
		}
		this.#peaks.set(id, out);
		return out;
	}

	/**
	 * Play the clips from scene frame `frame` (for `seconds`, or to their ends). Stops
	 * what was playing first.
	 */
	play(clips: AudioClip[], frame: number, fps: number, seconds?: number) {
		this.stop();
		const ctx = this.ctx();
		const t0 = ctx.currentTime + 0.02;
		for (const c of clips) {
			const b = this.#sounds.get(c.file)?.buffer;
			if (!b || c.muted || c.gain <= 0) continue;
			const at = (frame - c.start) / fps;
			if (at >= b.duration) continue;
			if (seconds !== undefined && -at >= seconds) continue;
			const src = ctx.createBufferSource();
			src.buffer = b;
			const gain = ctx.createGain();
			gain.gain.value = c.gain;
			src.connect(gain).connect(ctx.destination);
			const when = t0 + Math.max(0, -at);
			const offset = Math.max(0, at);
			if (seconds !== undefined) {
				// a snippet: a few milliseconds of fade so it doesn't click
				const len = Math.min(seconds - Math.max(0, -at), b.duration - offset);
				gain.gain.setValueAtTime(0, when);
				gain.gain.linearRampToValueAtTime(c.gain, when + 0.005);
				gain.gain.setValueAtTime(c.gain, when + Math.max(0.005, len - 0.01));
				gain.gain.linearRampToValueAtTime(0, when + len);
				src.start(when, offset, len);
			} else src.start(when, offset);
			this.#playing.push({ src, gain });
		}
	}

	/** A snippet around a frame while scrubbing: about two frames' worth, at least 1/12 s. */
	scrub(clips: AudioClip[], frame: number, fps: number) {
		this.play(clips, frame, fps, Math.max(2 / fps, 1 / 12));
	}

	stop() {
		for (const p of this.#playing) {
			try {
				p.src.stop();
			} catch {
				/* already stopped */
			}
			p.src.disconnect();
		}
		this.#playing = [];
	}

	/**
	 * The sound of a stretch of the film, mixed (for a video export): `parts` play one
	 * after another, each its scene's clips from a scene frame for some frames.
	 */
	async mixdown(
		parts: { clips: AudioClip[]; from: number; frames: number }[],
		fps: number,
		rate = 48000
	): Promise<AudioBuffer | null> {
		const total = parts.reduce((n, p) => n + p.frames, 0);
		const audible = parts.some((p) => p.clips.some((c) => !c.muted && this.#sounds.has(c.file)));
		if (!total || !audible) return null;
		const len = Math.max(1, Math.round((total / fps) * rate));
		const off = new OfflineAudioContext(2, len, rate);
		let t = 0;
		for (const p of parts) {
			const dur = p.frames / fps;
			for (const c of p.clips) {
				const b = this.#sounds.get(c.file)?.buffer;
				if (!b || c.muted || c.gain <= 0) continue;
				const at = (p.from - c.start) / fps;
				const offset = Math.max(0, at);
				const delay = Math.max(0, -at);
				const play = Math.min(dur - delay, b.duration - offset);
				if (play <= 0) continue;
				const src = off.createBufferSource();
				src.buffer = b;
				const g = off.createGain();
				g.gain.value = c.gain;
				src.connect(g).connect(off.destination);
				src.start(t + delay, offset, play);
			}
			t += dur;
		}
		return off.startRendering();
	}
}

/** An audio buffer as a 16-bit stereo WAV file. */
export function wav(b: AudioBuffer): Uint8Array {
	const ch = 2;
	const n = b.length;
	const out = new Uint8Array(44 + n * ch * 2);
	const v = new DataView(out.buffer);
	const str = (at: number, s: string) =>
		[...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
	str(0, 'RIFF');
	v.setUint32(4, 36 + n * ch * 2, true);
	str(8, 'WAVE');
	str(12, 'fmt ');
	v.setUint32(16, 16, true);
	v.setUint16(20, 1, true);
	v.setUint16(22, ch, true);
	v.setUint32(24, b.sampleRate, true);
	v.setUint32(28, b.sampleRate * ch * 2, true);
	v.setUint16(32, ch * 2, true);
	v.setUint16(34, 16, true);
	str(36, 'data');
	v.setUint32(40, n * ch * 2, true);
	const l = b.getChannelData(0);
	const r = b.numberOfChannels > 1 ? b.getChannelData(1) : l;
	let at = 44;
	for (let i = 0; i < n; i++) {
		for (const d of [l, r]) {
			const s = Math.max(-1, Math.min(1, d[i]));
			v.setInt16(at, s < 0 ? s * 0x8000 : s * 0x7fff, true);
			at += 2;
		}
	}
	return out;
}
