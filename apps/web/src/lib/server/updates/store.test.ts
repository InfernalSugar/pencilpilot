import { mkdtemp, readdir, rm, utimes, writeFile, mkdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { createHash } from 'node:crypto';
import { createServer, type Server } from 'node:http';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
	compareVersions,
	currentRelease,
	publish,
	putBlob,
	releaseFile,
	sweep,
	updaterManifest
} from './store';
import { createUpdatesHandler } from './http';

let dir: string;
beforeEach(async () => (dir = await mkdtemp(join(tmpdir(), 'pp-updates-'))));
afterEach(() => rm(dir, { recursive: true, force: true }));

const sha = (s: string) => createHash('sha256').update(s).digest('hex');
const upload = (s: string) => putBlob(dir, Readable.from([Buffer.from(s)]), { maxBytes: 1000 });

/** a release of one installer (its content `body`) */
async function release(version: string, body = `app ${version}`) {
	const b = await upload(body);
	return publish(
		dir,
		{
			version,
			notes: `What's new in ${version}`,
			files: [{ name: `PencilPilot_${version}.AppImage`, sha256: b.sha256 }],
			platforms: {
				'linux-x86_64': { file: `PencilPilot_${version}.AppImage`, signature: 'sig' }
			}
		},
		2
	);
}

/** make a file look `hours` old */
const age = (path: string, hours: number) => {
	const t = new Date(Date.now() - hours * 3600_000);
	return utimes(path, t, t);
};

describe('versions', () => {
	it('orders as semver: previews before their release, numbers by value', () => {
		const sorted = ['0.10.0', '0.2.0', '0.2.0-preview.10', '0.2.0-preview.2', '1.0.0-rc.1'].sort(
			compareVersions
		);
		expect(sorted).toEqual([
			'0.2.0-preview.2',
			'0.2.0-preview.10',
			'0.2.0',
			'0.10.0',
			'1.0.0-rc.1'
		]);
		expect(compareVersions('1.0.0', '1.0.0')).toBe(0);
	});
});

describe('blobs', () => {
	it('are named by their content, stored once', async () => {
		const a = await upload('hello');
		const b = await upload('hello');
		expect(a).toEqual({ sha256: sha('hello'), size: 5 });
		expect(b.sha256).toBe(a.sha256);
		expect(await readdir(join(dir, 'blobs'))).toEqual([a.sha256]);
		expect(await readdir(join(dir, 'incoming'))).toEqual([]);
	});

	it('refuses a wrong checksum or too big a file, leaving nothing behind', async () => {
		await expect(
			putBlob(dir, Readable.from([Buffer.from('x')]), { maxBytes: 10, expect: sha('y') })
		).rejects.toThrow(/mismatch/);
		await expect(putBlob(dir, Readable.from([Buffer.alloc(11)]), { maxBytes: 10 })).rejects.toThrow(
			/big/
		);
		expect(await readdir(join(dir, 'blobs'))).toEqual([]);
		expect(await readdir(join(dir, 'incoming'))).toEqual([]);
	});
});

describe('publishing', () => {
	it('offers the release just published, with download links', async () => {
		await release('0.1.0');
		await release('0.2.0');
		const r = await currentRelease(dir);
		expect(r?.version).toBe('0.2.0');
		expect(updaterManifest(r!, 'https://pencilpilot.app')).toEqual({
			version: '0.2.0',
			notes: "What's new in 0.2.0",
			pub_date: r!.pub_date,
			platforms: {
				'linux-x86_64': {
					signature: 'sig',
					url: 'https://pencilpilot.app/updates/download/0.2.0/PencilPilot_0.2.0.AppImage'
				}
			}
		});
		expect((await releaseFile(dir, '0.2.0', 'PencilPilot_0.2.0.AppImage'))?.file.size).toBe(9);
	});

	it('publishing an older version again rolls back to it', async () => {
		await release('0.1.0');
		await release('0.2.0');
		await release('0.1.0');
		expect((await currentRelease(dir))?.version).toBe('0.1.0');
	});

	it('refuses files not uploaded, unsafe names, unknown platforms', async () => {
		const b = await upload('x');
		const base = { version: '1.0.0', platforms: {} };
		await expect(
			publish(dir, { ...base, files: [{ name: 'a.exe', sha256: sha('nope') }] }, 3)
		).rejects.toThrow(/Not uploaded/);
		await expect(
			publish(dir, { ...base, files: [{ name: '../a.exe', sha256: b.sha256 }] }, 3)
		).rejects.toThrow(/Bad file/);
		await expect(
			publish(
				dir,
				{
					...base,
					files: [{ name: 'a.exe', sha256: b.sha256 }],
					platforms: { 'beos-x86_64': { file: 'a.exe', signature: 's' } }
				},
				3
			)
		).rejects.toThrow(/Bad platform/);
		await expect(publish(dir, { ...base, version: 'latest', files: [] }, 3)).rejects.toThrow(
			/Bad version/
		);
		expect(await currentRelease(dir)).toBeNull();
	});
});

describe('the orphan sweep', () => {
	it('keeps the newest releases and the current one; deletes the rest and their files', async () => {
		await release('0.1.0');
		await release('0.2.0');
		await release('0.3.0');
		// keep 2: 0.3.0 and 0.2.0; 0.1.0 and its installer go
		expect(await releaseFile(dir, '0.1.0', 'PencilPilot_0.1.0.AppImage')).toBeNull();
		expect((await readdir(join(dir, 'releases'))).sort()).toEqual(['0.2.0.json', '0.3.0.json']);
		expect((await readdir(join(dir, 'blobs'))).sort()).toEqual(
			[sha('app 0.2.0'), sha('app 0.3.0')].sort()
		);
		// rolled back to 0.2.0: kept, though keeping only the newest one
		await release('0.2.0');
		const r = await sweep(dir, { keep: 1 });
		expect(r.releases).toEqual([]);
		expect((await currentRelease(dir))?.version).toBe('0.2.0');
		// then 0.4.0 (publishing keeps 2: 0.4.0 and 0.3.0), and a sweep keeping 1
		await release('0.4.0');
		expect((await readdir(join(dir, 'releases'))).sort()).toEqual(['0.3.0.json', '0.4.0.json']);
		expect((await sweep(dir, { keep: 1 })).releases).toEqual(['0.3.0']);
		expect(await readdir(join(dir, 'blobs'))).toEqual([sha('app 0.4.0')]);
	});

	it('leaves a fresh upload waiting for its release, deletes an old unclaimed one', async () => {
		await release('0.1.0');
		const fresh = await upload('waiting');
		const stale = await upload('forgotten');
		await age(join(dir, 'blobs', stale.sha256), 7);
		const r = await sweep(dir, { keep: 3 });
		expect(r.blobs).toEqual([stale.sha256]);
		expect(await readdir(join(dir, 'blobs'))).toContain(fresh.sha256);
	});

	it('deletes stalled uploads and stray files', async () => {
		await mkdir(join(dir, 'incoming'), { recursive: true });
		await mkdir(join(dir, 'blobs'), { recursive: true });
		await writeFile(join(dir, 'incoming', 'old.part'), 'x');
		await writeFile(join(dir, 'incoming', 'new.part'), 'x');
		await writeFile(join(dir, 'blobs', 'not-a-hash'), 'x');
		await age(join(dir, 'incoming', 'old.part'), 2);
		const r = await sweep(dir, { keep: 3 });
		expect(r.parts).toEqual(['old.part']);
		expect(r.blobs).toEqual(['not-a-hash']);
		expect(await readdir(join(dir, 'incoming'))).toEqual(['new.part']);
	});
});

describe('over HTTP', () => {
	let server: Server;
	let base: string;
	beforeEach(async () => {
		const serve = createUpdatesHandler({ dir, token: 'secret', keep: 3, maxBytes: 1000 });
		server = createServer((req, res) => {
			if (!serve(req, res)) res.writeHead(418).end();
		});
		await new Promise<void>((ok) => server.listen(0, '127.0.0.1', ok));
		const a = server.address();
		base = `http://127.0.0.1:${typeof a === 'object' && a ? a.port : 0}`;
	});
	afterEach(() => new Promise<void>((ok) => server.close(() => ok())));

	const auth = { Authorization: 'Bearer secret' };

	it('uploads, publishes, then the app finds and downloads the update', async () => {
		expect((await fetch(`${base}/updates/latest.json`)).status).toBe(404);
		const put = await fetch(`${base}/updates/api/blobs`, {
			method: 'PUT',
			headers: { ...auth, 'X-Sha256': sha('installer') },
			body: 'installer'
		});
		expect(await put.json()).toEqual({ sha256: sha('installer'), size: 9 });
		const pub = await fetch(`${base}/updates/api/releases`, {
			method: 'POST',
			headers: auth,
			body: JSON.stringify({
				version: '0.2.0',
				notes: 'n',
				files: [{ name: 'PencilPilot_0.2.0_x64-setup.exe', sha256: sha('installer') }],
				platforms: {
					'windows-x86_64': { file: 'PencilPilot_0.2.0_x64-setup.exe', signature: 'sig' }
				}
			})
		});
		expect(pub.status).toBe(200);
		const latest = await (await fetch(`${base}/updates/latest.json`)).json();
		expect(latest.version).toBe('0.2.0');
		const file = await fetch(latest.platforms['windows-x86_64'].url);
		expect(await file.text()).toBe('installer');
		expect((await fetch(`${base}/elsewhere`)).status).toBe(418);
	});

	it('turns away uploads without the token', async () => {
		const r = await fetch(`${base}/updates/api/blobs`, { method: 'PUT', body: 'x' });
		expect(r.status).toBe(401);
		const wrong = await fetch(`${base}/updates/api/sweep`, {
			method: 'POST',
			headers: { Authorization: 'Bearer secreT' }
		});
		expect(wrong.status).toBe(401);
	});

	it('is off without a token set', async () => {
		const off = createUpdatesHandler({ dir, keep: 3, maxBytes: 1000 });
		const s = createServer((req, res) => void off(req, res));
		await new Promise<void>((ok) => s.listen(0, '127.0.0.1', ok));
		const a = s.address();
		const port = typeof a === 'object' && a ? a.port : 0;
		const r = await fetch(`http://127.0.0.1:${port}/updates/api/sweep`, {
			method: 'POST',
			headers: auth
		});
		expect(r.status).toBe(503);
		await new Promise<void>((ok) => s.close(() => ok()));
	});
});
