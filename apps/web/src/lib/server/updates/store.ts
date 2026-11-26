/**
 * The app's updates, kept on this server's own disk (no outside storage):
 *
 *   <dir>/blobs/<sha256>          each installer once, named by its content
 *   <dir>/incoming/<id>.part      an upload still arriving
 *   <dir>/releases/<version>.json a release: its notes and which blob is which file
 *   <dir>/current.json            the release the app is offered ({ "version": … })
 *
 * The Release workflow uploads the installers (blobs) and then publishes the release that
 * names them. The sweep keeps the newest few releases (and the current one) and deletes
 * everything nothing points at any more: older releases, their blobs, stalled uploads.
 * Uploading the same file twice stores it once.
 */
import { createHash, randomBytes } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { mkdir, readdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { Transform } from 'node:stream';

/** A file of a release (an installer, or the updater's package of one). */
export interface ReleaseFile {
	name: string;
	sha256: string;
	size: number;
}

/** What the updater needs for one system: the signed package's file and its signature. */
export interface PlatformEntry {
	/** the file's name in `files` */
	file: string;
	/** the updater signature (the .sig file's text) */
	signature: string;
}

export interface Release {
	version: string;
	notes: string;
	/** RFC 3339 */
	pub_date: string;
	/** every file people can download (installers, checksums) */
	files: ReleaseFile[];
	/** updater keys, e.g. "windows-x86_64", "linux-x86_64-appimage", "darwin-aarch64" */
	platforms: Record<string, PlatformEntry>;
}

export const SHA256 = /^[0-9a-f]{64}$/;
/** semver: 1.2.3, 1.2.3-preview.4 (build metadata isn't used) */
const SEMVER =
	/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;
/** a file name to serve: no folders, no dot files */
const FILE_NAME = /^[A-Za-z0-9_+][A-Za-z0-9._+-]{0,199}$/;
/** os-arch, optionally -installer (the updater's own keys) */
const PLATFORM = /^(linux|windows|darwin)-(x86_64|aarch64|i686|armv7)(-[a-z]+)?$/;

export const isVersion = (v: unknown): v is string => typeof v === 'string' && SEMVER.test(v);
export const isFileName = (n: unknown): n is string =>
	typeof n === 'string' && FILE_NAME.test(n) && !n.includes('..');

/** Order two versions as semver does (a preview comes before its release). */
export function compareVersions(a: string, b: string): number {
	const pa = SEMVER.exec(a);
	const pb = SEMVER.exec(b);
	if (!pa || !pb) throw new Error(`not a version: ${pa ? b : a}`);
	for (let i = 1; i <= 3; i++) {
		const d = Number(pa[i]) - Number(pb[i]);
		if (d) return Math.sign(d);
	}
	const [ra, rb] = [pa[4], pb[4]];
	if (!ra || !rb) return ra === rb ? 0 : ra ? -1 : 1;
	const xa = ra.split('.');
	const xb = rb.split('.');
	for (let i = 0; i < Math.max(xa.length, xb.length); i++) {
		if (xa[i] === undefined) return -1;
		if (xb[i] === undefined) return 1;
		const na = /^\d+$/.test(xa[i]);
		const nb = /^\d+$/.test(xb[i]);
		if (na && nb) {
			const d = Number(xa[i]) - Number(xb[i]);
			if (d) return Math.sign(d);
		} else if (na !== nb) return na ? -1 : 1;
		else if (xa[i] !== xb[i]) return xa[i] < xb[i] ? -1 : 1;
	}
	return 0;
}

export class UpdateError extends Error {
	constructor(
		readonly status: number,
		message: string
	) {
		super(message);
	}
}

const blobPath = (dir: string, sha: string) => join(dir, 'blobs', sha);
const releasePath = (dir: string, version: string) => join(dir, 'releases', `${version}.json`);

/** Write a file whole or not at all (a reader never sees half of it). */
async function writeAtomic(path: string, text: string) {
	const tmp = `${path}.${randomBytes(6).toString('hex')}.tmp`;
	await writeFile(tmp, text);
	await rename(tmp, path);
}

async function exists(path: string) {
	try {
		await stat(path);
		return true;
	} catch {
		return false;
	}
}

/**
 * Store an upload as a blob: streamed to disk while it's hashed, so a large installer never
 * sits in memory. `expect` (its sha256, when the uploader sends it) must match.
 */
export async function putBlob(
	dir: string,
	body: Readable,
	opts: { maxBytes: number; expect?: string }
): Promise<{ sha256: string; size: number }> {
	await mkdir(join(dir, 'incoming'), { recursive: true });
	await mkdir(join(dir, 'blobs'), { recursive: true });
	const part = join(dir, 'incoming', `${randomBytes(12).toString('hex')}.part`);
	const hash = createHash('sha256');
	let size = 0;
	const count = new Transform({
		transform(chunk: Buffer, _enc, done) {
			size += chunk.length;
			if (size > opts.maxBytes) return done(new UpdateError(413, 'Too big'));
			hash.update(chunk);
			done(null, chunk);
		}
	});
	try {
		await pipeline(body, count, createWriteStream(part, { flags: 'wx' }));
	} catch (e) {
		await rm(part, { force: true });
		throw e instanceof UpdateError ? e : new UpdateError(400, 'The upload broke off');
	}
	const sha256 = hash.digest('hex');
	if (opts.expect && opts.expect !== sha256) {
		await rm(part, { force: true });
		throw new UpdateError(400, `Checksum mismatch: got ${sha256}`);
	}
	const blob = blobPath(dir, sha256);
	if (await exists(blob)) await rm(part, { force: true });
	else await rename(part, blob);
	return { sha256, size };
}

/** Check a release from the uploader: every name safe, every file uploaded, every platform a file. */
async function cleanRelease(dir: string, input: unknown): Promise<Release> {
	const r = input as Partial<Release> | null;
	if (!r || typeof r !== 'object') throw new UpdateError(400, 'Not a release');
	if (!isVersion(r.version)) throw new UpdateError(400, 'Bad version');
	const notes = typeof r.notes === 'string' ? r.notes.slice(0, 20_000) : '';
	const pub_date =
		typeof r.pub_date === 'string' && !Number.isNaN(Date.parse(r.pub_date))
			? new Date(r.pub_date).toISOString()
			: new Date().toISOString();
	if (!Array.isArray(r.files) || r.files.length === 0 || r.files.length > 50)
		throw new UpdateError(400, 'Bad files');
	const files: ReleaseFile[] = [];
	const names = new Set<string>();
	for (const f of r.files) {
		if (!f || !isFileName(f.name) || typeof f.sha256 !== 'string' || !SHA256.test(f.sha256))
			throw new UpdateError(400, `Bad file: ${JSON.stringify(f?.name ?? f)}`);
		if (names.has(f.name)) throw new UpdateError(400, `Two files called ${f.name}`);
		let size: number;
		try {
			size = (await stat(blobPath(dir, f.sha256))).size;
		} catch {
			throw new UpdateError(409, `Not uploaded yet: ${f.name}`);
		}
		names.add(f.name);
		files.push({ name: f.name, sha256: f.sha256, size });
	}
	const platforms: Record<string, PlatformEntry> = {};
	for (const [key, p] of Object.entries(r.platforms ?? {})) {
		if (!PLATFORM.test(key)) throw new UpdateError(400, `Bad platform: ${key}`);
		if (!p || !names.has(p.file)) throw new UpdateError(400, `${key}: no such file`);
		if (typeof p.signature !== 'string' || !p.signature.trim() || p.signature.length > 4000)
			throw new UpdateError(400, `${key}: bad signature`);
		platforms[key] = { file: p.file, signature: p.signature.trim() };
	}
	return { version: r.version, notes, pub_date, files, platforms };
}

/**
 * Publish a release (its files already uploaded) and offer it to the app. Publishing the
 * same version again replaces it; an older version goes back to being offered, which
 * undoes a bad release. Sweeps afterwards.
 */
export async function publish(dir: string, input: unknown, keep: number): Promise<Release> {
	const release = await cleanRelease(dir, input);
	await mkdir(join(dir, 'releases'), { recursive: true });
	await writeAtomic(releasePath(dir, release.version), JSON.stringify(release, null, '\t'));
	await writeAtomic(join(dir, 'current.json'), JSON.stringify({ version: release.version }));
	await sweep(dir, { keep });
	return release;
}

export async function readRelease(dir: string, version: string): Promise<Release | null> {
	if (!isVersion(version)) return null;
	try {
		return JSON.parse(await readFile(releasePath(dir, version), 'utf8')) as Release;
	} catch {
		return null;
	}
}

/** The release the app is offered (null: none published yet). */
export async function currentRelease(dir: string): Promise<Release | null> {
	try {
		const { version } = JSON.parse(await readFile(join(dir, 'current.json'), 'utf8'));
		return await readRelease(dir, version);
	} catch {
		return null;
	}
}

/** A release's file on disk, or null. */
export async function releaseFile(
	dir: string,
	version: string,
	name: string
): Promise<{ path: string; file: ReleaseFile } | null> {
	if (!isFileName(name)) return null;
	const r = await readRelease(dir, version);
	const file = r?.files.find((f) => f.name === name);
	return file ? { path: blobPath(dir, file.sha256), file } : null;
}

/** The updater's manifest (Tauri's "static JSON" format), download links under `base`. */
export function updaterManifest(r: Release, base: string) {
	const platforms: Record<string, { signature: string; url: string }> = {};
	for (const [key, p] of Object.entries(r.platforms)) {
		platforms[key] = {
			signature: p.signature,
			url: `${base}/updates/download/${r.version}/${encodeURIComponent(p.file)}`
		};
	}
	return { version: r.version, notes: r.notes, pub_date: r.pub_date, platforms };
}

export interface SweepReport {
	releases: string[];
	blobs: string[];
	parts: string[];
}

/**
 * The orphan sweep. Keeps the newest `keep` releases and the current one; deletes the
 * other releases, every blob none of the kept releases names, and stalled uploads. A blob
 * younger than `graceMs` stays even when nothing names it yet: the Release workflow
 * uploads every file before it publishes the release that names them.
 */
export async function sweep(
	dir: string,
	opts: { keep: number; now?: number; graceMs?: number; partMaxAgeMs?: number }
): Promise<SweepReport> {
	const now = opts.now ?? Date.now();
	const graceMs = opts.graceMs ?? 6 * 3600_000;
	const partMaxAgeMs = opts.partMaxAgeMs ?? 3600_000;
	const report: SweepReport = { releases: [], blobs: [], parts: [] };
	const list = async (sub: string) => readdir(join(dir, sub)).catch(() => [] as string[]);

	const current = (await currentRelease(dir))?.version;
	const versions = (await list('releases'))
		.filter((f) => f.endsWith('.json') && isVersion(f.slice(0, -5)))
		.map((f) => f.slice(0, -5))
		.sort((a, b) => compareVersions(b, a));
	const kept = new Set(versions.slice(0, Math.max(1, opts.keep)));
	if (current) kept.add(current);

	const referenced = new Set<string>();
	/** the files of releases dropped now: no grace, nothing is waiting to claim them */
	const dropped = new Set<string>();
	for (const v of versions) {
		const r = await readRelease(dir, v);
		// unreadable: keep every blob (and leave it for a person to look at)
		if (!r) return report;
		if (kept.has(v)) {
			for (const f of r.files) referenced.add(f.sha256);
			continue;
		}
		for (const f of r.files) dropped.add(f.sha256);
		await rm(releasePath(dir, v), { force: true });
		report.releases.push(v);
	}

	const old = async (path: string, age: number) => {
		try {
			return now - (await stat(path)).mtimeMs > age;
		} catch {
			return false;
		}
	};
	for (const b of await list('blobs')) {
		const path = join(dir, 'blobs', b);
		if (referenced.has(b)) continue;
		if (SHA256.test(b) && !dropped.has(b) && !(await old(path, graceMs))) continue;
		await rm(path, { force: true });
		report.blobs.push(b);
	}
	for (const p of await list('incoming')) {
		const path = join(dir, 'incoming', p);
		if (await old(path, partMaxAgeMs)) {
			await rm(path, { force: true });
			report.parts.push(p);
		}
	}
	// a write that never finished renaming
	for (const t of await list('releases')) {
		const path = join(dir, 'releases', t);
		if (t.endsWith('.tmp') && (await old(path, partMaxAgeMs))) await rm(path, { force: true });
	}
	return report;
}
