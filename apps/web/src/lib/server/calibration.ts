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
	writeFileSync,
	rmSync
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
	/** the grader's named measurements (lets `calibrate fit` tune weights, not just the curve) */
	measurements?: Record<string, number>;
	/** 'finished' or 'sketch' */
	mode?: string;
	/** a copy of the reference is kept with the sample (live pictures can disappear) */
	refFile?: string;
}

export interface Label {
	sample: string;
	category: string;
	similarity: number;
	score: number;
	human: number;
	measurements?: Record<string, number>;
	mode?: string;
}

const DATA_URL = /^data:image\/(png|jpeg|webp);base64,([A-Za-z0-9+/=]+)$/;

export function saveSample(
	s: Omit<Sample, 'id' | 'savedAt' | 'refFile'>,
	png: string,
	refImage?: string
): Sample {
	mkdirSync(SAMPLES, { recursive: true });
	const id = `s${Date.now()}`;
	const m = png.match(DATA_URL);
	if (!m || m[1] !== 'png') throw new Error('drawing must be a PNG data URL');
	writeFileSync(join(SAMPLES, `${id}.png`), Buffer.from(m[2], 'base64'));
	let refFile: string | undefined;
	const r = refImage?.match(DATA_URL);
	if (r) {
		refFile = `${id}-ref.${r[1] === 'jpeg' ? 'jpg' : r[1]}`;
		writeFileSync(join(SAMPLES, refFile), Buffer.from(r[2], 'base64'));
	}
	const sample: Sample = { ...s, id, savedAt: Date.now(), ...(refFile ? { refFile } : {}) };
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
		human,
		...(s.measurements ? { measurements: s.measurements } : {}),
		...(s.mode ? { mode: s.mode } : {})
	};
	mkdirSync(CAL_DIR, { recursive: true });
	appendFileSync(LABELS, JSON.stringify(l) + '\n');
	return l;
}

export const samplePng = (id: string) =>
	/^s\d+$/.test(id) && existsSync(join(SAMPLES, `${id}.png`))
		? readFileSync(join(SAMPLES, `${id}.png`))
		: null;

/** The kept copy of a sample's reference picture (bytes and content type), if any. */
export function sampleRef(id: string): { bytes: Buffer; type: string } | null {
	const s = samples().find((x) => x.id === id);
	if (!s?.refFile || !/^s\d+-ref\.(png|jpg|webp)$/.test(s.refFile)) return null;
	const p = join(SAMPLES, s.refFile);
	if (!existsSync(p)) return null;
	const ext = s.refFile.split('.').pop();
	return { bytes: readFileSync(p), type: ext === 'jpg' ? 'image/jpeg' : `image/${ext}` };
}

/** Drop a sample (a test scribble, a broken save) and its files. */
export function deleteSample(id: string) {
	if (!/^s\d+$/.test(id)) throw new Error('bad sample id');
	const s = samples().find((x) => x.id === id);
	if (!s) throw new Error('unknown sample');
	for (const f of [`${id}.png`, `${id}.json`, s.refFile].filter(Boolean) as string[])
		rmSync(join(SAMPLES, f), { force: true });
}
