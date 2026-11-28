/**
 * The app's updates over HTTP, in front of SvelteKit (it holds whole request bodies in
 * memory and stops at 512 KB; installers are 100 MB and more, so they stream to disk here).
 *
 *   GET  /updates/latest.json                     what the app checks (Tauri's updater format)
 *   GET  /updates/release.json                    the current release: notes and every file
 *   GET  /updates/download/<version>/<file>       a file of a kept release
 *   PUT  /updates/api/blobs        (token)        upload one file, body = the file
 *   POST /updates/api/releases     (token)        publish a release naming uploaded files
 *   POST /updates/api/sweep        (token)        run the orphan sweep now
 *
 * The token is UPDATES_TOKEN (the Release workflow has the same one as a secret); without
 * it the upload API is off. Files live in UPDATES_DIR (default <repo>/data/updates).
 */
import type { IncomingMessage, ServerResponse } from 'node:http';
import { createReadStream } from 'node:fs';
import { timingSafeEqual } from 'node:crypto';
import { resolve } from 'node:path';
import {
	SHA256,
	UpdateError,
	currentRelease,
	publish,
	putBlob,
	releaseFile,
	sweep,
	updaterManifest
} from './store';

export interface UpdatesConfig {
	dir: string;
	/** the upload token (unset: the upload API answers 503) */
	token?: string;
	/** the site's public address, for download links (unset: from the request) */
	origin?: string;
	/** releases kept (the newest few) */
	keep: number;
	/** largest file accepted */
	maxBytes: number;
}

export function updatesConfig(env: NodeJS.ProcessEnv = process.env): UpdatesConfig {
	const keep = Number(env.UPDATES_KEEP);
	return {
		dir: env.UPDATES_DIR || resolve(process.cwd(), '../../data/updates'),
		token: env.UPDATES_TOKEN || undefined,
		origin: (env.UPDATES_ORIGIN || env.ORIGIN || '').replace(/\/+$/, '') || undefined,
		keep: Number.isInteger(keep) && keep > 0 ? keep : 3,
		maxBytes: 1024 * 1024 * 1024
	};
}

const PREFIX = '/updates/';

function send(res: ServerResponse, status: number, body: unknown, headers = {}) {
	const text = typeof body === 'string' ? body : JSON.stringify(body);
	res.writeHead(status, {
		'Content-Type': typeof body === 'string' ? 'text/plain; charset=utf-8' : 'application/json',
		'Content-Length': Buffer.byteLength(text),
		'X-Content-Type-Options': 'nosniff',
		...headers
	});
	res.end(text);
}

function authorised(req: IncomingMessage, token: string) {
	const given = /^Bearer (.+)$/.exec(req.headers.authorization ?? '')?.[1] ?? '';
	const a = Buffer.from(given);
	const b = Buffer.from(token);
	return a.length === b.length && timingSafeEqual(a, b);
}

function origin(req: IncomingMessage, cfg: UpdatesConfig) {
	if (cfg.origin) return cfg.origin;
	const proto = String(req.headers['x-forwarded-proto'] ?? 'http')
		.split(',')[0]
		.trim();
	return `${proto}://${req.headers['x-forwarded-host'] ?? req.headers.host}`;
}

async function readJson(req: IncomingMessage, max: number): Promise<unknown> {
	let size = 0;
	const chunks: Buffer[] = [];
	for await (const c of req) {
		size += c.length;
		if (size > max) throw new UpdateError(413, 'Too big');
		chunks.push(c);
	}
	try {
		return JSON.parse(Buffer.concat(chunks).toString('utf8'));
	} catch {
		throw new UpdateError(400, 'Bad JSON');
	}
}

async function route(req: IncomingMessage, res: ServerResponse, cfg: UpdatesConfig, path: string) {
	const method = req.method ?? 'GET';
	const read = method === 'GET' || method === 'HEAD';

	if (path === '/updates/latest.json' && read) {
		const r = await currentRelease(cfg.dir);
		if (!r) return send(res, 404, 'No release yet');
		// a short cache: a new release reaches everyone within minutes
		return send(res, 200, updaterManifest(r, origin(req, cfg)), {
			'Cache-Control': 'public, max-age=300'
		});
	}
	if (path === '/updates/release.json' && read) {
		const r = await currentRelease(cfg.dir);
		if (!r) return send(res, 404, 'No release yet');
		const base = origin(req, cfg);
		return send(
			res,
			200,
			{
				version: r.version,
				notes: r.notes,
				pub_date: r.pub_date,
				files: r.files.map((f) => ({
					...f,
					url: `${base}/updates/download/${r.version}/${encodeURIComponent(f.name)}`
				}))
			},
			{ 'Cache-Control': 'public, max-age=300' }
		);
	}

	const dl = /^\/updates\/download\/([^/]+)\/([^/]+)$/.exec(path);
	if (dl && read) {
		let name: string;
		try {
			name = decodeURIComponent(dl[2]);
		} catch {
			return send(res, 404, 'Not found');
		}
		const found = await releaseFile(cfg.dir, dl[1], name);
		if (!found) return send(res, 404, 'Not found');
		res.writeHead(200, {
			'Content-Type': 'application/octet-stream',
			'Content-Length': found.file.size,
			'Content-Disposition': `attachment; filename="${found.file.name}"`,
			// a version's file never changes (its name says which version)
			'Cache-Control': 'public, max-age=31536000, immutable',
			ETag: `"${found.file.sha256}"`,
			'X-Content-Type-Options': 'nosniff'
		});
		if (method === 'HEAD') return res.end();
		const stream = createReadStream(found.path);
		stream.on('error', () => res.destroy());
		stream.pipe(res);
		return;
	}

	if (path.startsWith('/updates/api/')) {
		if (!cfg.token) return send(res, 503, 'Uploads are off (UPDATES_TOKEN is not set)');
		if (!authorised(req, cfg.token)) return send(res, 401, 'Not allowed');
		if (path === '/updates/api/blobs' && method === 'PUT') {
			const expect = String(req.headers['x-sha256'] ?? '').toLowerCase() || undefined;
			if (expect && !SHA256.test(expect)) return send(res, 400, 'Bad X-Sha256');
			const declared = Number(req.headers['content-length']);
			if (declared > cfg.maxBytes) return send(res, 413, 'Too big');
			return send(res, 200, await putBlob(cfg.dir, req, { maxBytes: cfg.maxBytes, expect }));
		}
		if (path === '/updates/api/releases' && method === 'POST') {
			const r = await publish(cfg.dir, await readJson(req, 1_000_000), cfg.keep);
			return send(res, 200, { version: r.version, files: r.files.length });
		}
		if (path === '/updates/api/sweep' && method === 'POST') {
			return send(res, 200, await sweep(cfg.dir, { keep: cfg.keep }));
		}
	}
	send(res, 404, 'Not found');
}

/**
 * Handles /updates/… and answers true; anything else is left (false) for the site.
 */
export function createUpdatesHandler(cfg: UpdatesConfig = updatesConfig()) {
	return (req: IncomingMessage, res: ServerResponse): boolean => {
		const path = new URL(req.url ?? '/', 'http://x').pathname;
		if (!path.startsWith(PREFIX)) return false;
		route(req, res, cfg, path).catch((e) => {
			const status = e instanceof UpdateError ? e.status : 500;
			if (status === 500) console.error('updates:', e);
			if (res.headersSent) res.destroy();
			else send(res, status, status === 500 ? 'Something went wrong' : e.message);
			// stop reading an upload that was turned down
			if (!req.complete) req.destroy();
		});
		return true;
	};
}

/** The sweep every hour too (an upload that never got its release, a crash mid-publish). */
export function startSweeping(cfg: UpdatesConfig = updatesConfig()) {
	const run = () =>
		sweep(cfg.dir, { keep: cfg.keep })
			.then((r) => {
				const n = r.releases.length + r.blobs.length + r.parts.length;
				if (n) console.log(`updates: swept ${JSON.stringify(r)}`);
			})
			.catch((e) => console.error('updates: sweep failed', e));
	void run();
	const t = setInterval(run, 3600_000);
	t.unref();
	return () => clearInterval(t);
}
