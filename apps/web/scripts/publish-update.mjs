#!/usr/bin/env node
// Put a release of the app on the website, for the app's self-update:
// uploads each file (streamed; the site stores it once by its checksum), then publishes
// the release naming them. The site's orphan sweep then drops the oldest releases.
//
//   UPDATES_URL=https://pencilpilot.app UPDATES_TOKEN=… \
//     node apps/web/scripts/publish-update.mjs --version 0.2.0 --notes notes.txt <folder>
//
// <folder> holds the installers, and the updater's packages and their .sig files, as the
// App workflow builds them (flat or in its bundle/<kind>/ folders). Run by the Release workflow.
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readdir, readFile, stat } from 'node:fs/promises';
import { basename, join } from 'node:path';
import { Readable } from 'node:stream';
import { parseArgs } from 'node:util';

const { values, positionals } = parseArgs({
	options: {
		version: { type: 'string' },
		notes: { type: 'string' },
		arch: { type: 'string', default: '' },
		'dry-run': { type: 'boolean', default: false }
	},
	allowPositionals: true
});
const base = (process.env.UPDATES_URL ?? '').replace(/\/+$/, '');
const token = process.env.UPDATES_TOKEN ?? '';
const [folder] = positionals;
if (!values.version || !folder || (!values['dry-run'] && (!base || !token))) {
	console.error(
		'usage: UPDATES_URL=… UPDATES_TOKEN=… publish-update.mjs --version X [--notes file] <folder>'
	);
	process.exit(2);
}

async function walk(dir) {
	const out = [];
	for (const e of await readdir(dir, { withFileTypes: true })) {
		const p = join(dir, e.name);
		if (e.isDirectory()) out.push(...(await walk(p)));
		else out.push(p);
	}
	return out;
}

const all = await walk(folder);
/** what people download, and the updater's packages (each with a signature) */
const isFile = (n) =>
	/\.(exe|msi|deb|rpm|AppImage|dmg)$/.test(n) ||
	n.endsWith('.app.tar.gz') ||
	n === 'SHA256SUMS.txt';
const files = all.filter((p) => isFile(basename(p)));
const sigs = new Map(
	all.filter((p) => p.endsWith('.sig')).map((p) => [basename(p).slice(0, -4), p])
);

/** The updater's keys for a package (Tauri looks for <os>-<arch>-<installer>, then <os>-<arch>). */
function platformsFor(name) {
	// Linux and Windows names carry the arch; macOS's (PencilPilot.app.tar.gz) doesn't, and
	// the App workflow builds it on Apple silicon
	const arm = /aarch64|arm64/.test(name);
	const a = values.arch || (arm ? 'aarch64' : 'x86_64');
	if (name.endsWith('.AppImage')) return [`linux-${a}`, `linux-${a}-appimage`];
	if (name.endsWith('.deb')) return [`linux-${a}-deb`];
	if (name.endsWith('.rpm')) return [`linux-${a}-rpm`];
	if (name.endsWith('-setup.exe')) return [`windows-${a}`, `windows-${a}-nsis`];
	if (name.endsWith('.msi')) return [`windows-${a}-msi`];
	if (name.endsWith('.app.tar.gz')) {
		const m = values.arch || (/x64|x86_64/.test(name) ? 'x86_64' : 'aarch64');
		return [`darwin-${m}`, `darwin-${m}-app`];
	}
	return [];
}

const sha256 = (path) =>
	new Promise((ok, fail) => {
		const h = createHash('sha256');
		createReadStream(path)
			.on('data', (c) => h.update(c))
			.on('end', () => ok(h.digest('hex')))
			.on('error', fail);
	});

const release = { version: values.version, notes: '', files: [], platforms: {} };
if (values.notes) release.notes = (await readFile(values.notes, 'utf8')).trim();

for (const path of files) {
	const name = basename(path);
	const sum = await sha256(path);
	const { size } = await stat(path);
	console.log(
		`${values['dry-run'] ? 'would upload' : 'uploading'} ${name} (${(size / 1048576).toFixed(1)} MB)`
	);
	if (!values['dry-run']) {
		const r = await fetch(`${base}/updates/api/blobs`, {
			method: 'PUT',
			headers: {
				Authorization: `Bearer ${token}`,
				'X-Sha256': sum,
				'Content-Type': 'application/octet-stream',
				'Content-Length': String(size)
			},
			body: Readable.toWeb(createReadStream(path)),
			duplex: 'half'
		});
		if (!r.ok) throw new Error(`${name}: ${r.status} ${await r.text()}`);
	}
	release.files.push({ name, sha256: sum });
	const sig = sigs.get(name);
	if (sig) {
		const signature = (await readFile(sig, 'utf8')).trim();
		for (const key of platformsFor(name)) release.platforms[key] = { file: name, signature };
	}
}

if (!release.files.length) throw new Error(`No installers in ${folder}`);
if (!Object.keys(release.platforms).length)
	throw new Error(
		'No signed updater packages (.sig): was TAURI_SIGNING_PRIVATE_KEY set for the build?'
	);
console.log(`platforms: ${Object.keys(release.platforms).sort().join(', ')}`);
if (values['dry-run']) process.exit(0);

const r = await fetch(`${base}/updates/api/releases`, {
	method: 'POST',
	headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
	body: JSON.stringify(release)
});
if (!r.ok) throw new Error(`publishing: ${r.status} ${await r.text()}`);
console.log(`published ${values.version}: ${base}/updates/latest.json`);
