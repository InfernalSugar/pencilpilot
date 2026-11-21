#!/usr/bin/env node
/**
 * Fetch Rhubarb Lip Sync (MIT, github.com/DanielSWolf/rhubarb-lip-sync), the app's
 * lip-sync, for this machine, and check it against a pinned SHA-256. Its program, its
 * speech models (res/sphinx, which it looks for next to itself) and its licence go to
 * src-tauri/rhubarb/, where `tauri build --config src-tauri/tauri.bundle.conf.json`
 * takes the folder into the app's resources (and `tauri dev` finds it too).
 *
 * Rhubarb's macOS build is for Intel Macs: on Apple silicon it runs under Rosetta.
 * To update: change the version and hashes below.
 *
 *   node scripts/fetch-rhubarb.mjs          (from apps/studio)
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
	chmodSync,
	cpSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	writeFileSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = '1.14.0';
const BUILDS = {
	'x86_64-unknown-linux-gnu': {
		os: 'Linux',
		sha256: 'a9a9074862cff47b2d59b8bf399a678a3b0b74f9452ad6ad94cb292913dd8667'
	},
	'x86_64-pc-windows-msvc': {
		os: 'Windows',
		sha256: '62fa416a8d5e382a3828ee4bef358ce520d0b4cabdeaea75a7ac266d098d1fe3'
	},
	'aarch64-apple-darwin': {
		os: 'macOS',
		sha256: 'f991deacac6c973a14a4431a16a58b842f436531e120cfaea142c87c0d3ab4c5'
	},
	'x86_64-apple-darwin': {
		os: 'macOS',
		sha256: 'f991deacac6c973a14a4431a16a58b842f436531e120cfaea142c87c0d3ab4c5'
	}
};

const here = dirname(fileURLToPath(import.meta.url));
const target = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (\S+)/m)?.[1];
const build = target && BUILDS[target];
if (!build) {
	console.error(`No Rhubarb build is pinned for ${target ?? 'this machine'}.`);
	process.exit(1);
}
const win = build.os === 'Windows';
const out = join(here, '..', 'src-tauri', 'rhubarb');
const exe = win ? 'rhubarb.exe' : 'rhubarb';
if (existsSync(join(out, exe)) && existsSync(join(out, 'res', 'sphinx'))) {
	console.log(`${out} is already here.`);
	process.exit(0);
}

const name = `Rhubarb-Lip-Sync-${VERSION}-${build.os}`;
const url = `https://github.com/DanielSWolf/rhubarb-lip-sync/releases/download/v${VERSION}/${name}.zip`;
console.log(`Fetching ${url}`);
const res = await fetch(url, { redirect: 'follow' });
if (!res.ok) throw new Error(`${res.status} ${res.statusText}: ${url}`);
const bytes = Buffer.from(await res.arrayBuffer());
const sum = createHash('sha256').update(bytes).digest('hex');
if (sum !== build.sha256) throw new Error(`SHA-256 mismatch: got ${sum}, want ${build.sha256}`);

const work = mkdtempSync(join(tmpdir(), 'pp-rhubarb-'));
try {
	const archive = join(work, `${name}.zip`);
	writeFileSync(archive, bytes);
	// (bsdtar reads zips on Windows and macOS; Linux has unzip)
	if (process.platform === 'linux') execFileSync('unzip', ['-q', archive, '-d', work]);
	else execFileSync('tar', ['-xf', archive, '-C', work], { stdio: 'inherit' });
	const from = join(work, name);
	rmSync(out, { recursive: true, force: true });
	mkdirSync(out, { recursive: true });
	cpSync(join(from, exe), join(out, exe));
	if (!win) chmodSync(join(out, exe), 0o755);
	cpSync(join(from, 'res', 'sphinx'), join(out, 'res', 'sphinx'), { recursive: true });
	cpSync(join(from, 'LICENSE.md'), join(out, 'LICENSE.md'));
	console.log(`Rhubarb ${VERSION} for ${target} → ${out}`);
} finally {
	rmSync(work, { recursive: true, force: true });
}
