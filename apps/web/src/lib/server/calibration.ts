/**
 * Human labels for calibrating the grader (M8-T01), dev only. Samples (drawing
 * PNG + grade) are saved from the results page; you rate them 1–10 on /dev/rate;
 * `cargo run -p calibrate -- fit` fits the score curve to the labels.
 * Everything lives in <repo>/data/calibration (git-ignored).
 */
import {
	appendFileSync,
	existsSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	writeFileSync
} from 'node:fs';
import { join, resolve } from 'node:path';

export const CAL_DIR = resolve(process.cwd(), '../../data/calibration');
const SAMPLES = join(CAL_DIR, 'samples');
const LABELS = join(CAL_DIR, 'labels.jsonl');

export interface Sample {
	id: string;
	category: string;
	refId: string;
	refUrl: string;
	similarity: number;
	score: number;
	savedAt: number;
}

export interface Label {
	sample: string;
	category: string;
	similarity: number;
	score: number;
	human: number;
}

export function saveSample(s: Omit<Sample, 'id' | 'savedAt'>, png: string): Sample {
	mkdirSync(SAMPLES, { recursive: true });
	const id = `s${Date.now()}`;
	const m = png.match(/^data:image\/png;base64,([A-Za-z0-9+/=]+)$/);
	if (!m) throw new Error('drawing must be a PNG data URL');
	writeFileSync(join(SAMPLES, `${id}.png`), Buffer.from(m[1], 'base64'));
	const sample: Sample = { ...s, id, savedAt: Date.now() };
	writeFileSync(join(SAMPLES, `${id}.json`), JSON.stringify(sample));
	return sample;
}

export function labels(): Label[] {
	if (!existsSync(LABELS)) return [];
	return readFileSync(LABELS, 'utf8')
		.split('\n')
		.filter(Boolean)
		.map((l) => JSON.parse(l) as Label);
}

export function samples(): Sample[] {
	if (!existsSync(SAMPLES)) return [];
	return readdirSync(SAMPLES)
		.filter((f) => f.endsWith('.json'))
		.map((f) => JSON.parse(readFileSync(join(SAMPLES, f), 'utf8')) as Sample)
		.sort((a, b) => a.savedAt - b.savedAt);
}

export function addLabel(sampleId: string, human: number) {
	const s = samples().find((x) => x.id === sampleId);
	if (!s) throw new Error('unknown sample');
	if (!(human >= 1 && human <= 10)) throw new Error('rating must be 1–10');
	const l: Label = {
		sample: s.id,
		category: s.category,
		similarity: s.similarity,
		score: s.score,
		human
	};
	mkdirSync(CAL_DIR, { recursive: true });
	appendFileSync(LABELS, JSON.stringify(l) + '\n');
	return l;
}

export const samplePng = (id: string) =>
	/^s\d+$/.test(id) && existsSync(join(SAMPLES, `${id}.png`))
		? readFileSync(join(SAMPLES, `${id}.png`))
		: null;
