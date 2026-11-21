#!/usr/bin/env node
/**
 * Fetch the ffmpeg build bundled into the app's installers, for this machine's Rust
 * target, and check it against a pinned SHA-256. It goes to
 * src-tauri/binaries/pencilpilot-ffmpeg-<target>[.exe], where `tauri build --config
 * src-tauri/tauri.ffmpeg.conf.json` picks it up (named so it never clashes with a
 * system ffmpeg in /usr/bin).
 *
 * The builds are GPL v3 (with libx264, prores_ks, libvpx); src-tauri/ffmpeg/NOTICE.md
 * says where their sources are. To update: pick newer builds, change the URLs and
 * hashes below, and the versions in NOTICE.md.
 *
 *   node scripts/fetch-ffmpeg.mjs          (from apps/studio)
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
	chmodSync,
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readdirSync,
	rmSync,
	statSync,
	writeFileSync
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const BTBN = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-31-12-59';
const BUILDS = {
	'x86_64-unknown-linux-gnu': {
		url: `${BTBN}/ffmpeg-n9.0.2-39-g7adfbe40dc-linux64-gpl-9.0.tar.xz`,
		sha256: '73e29a56974ae5a651133a04584c7483b21afe941e17dc91d79814a758b608d3',
		exe: 'ffmpeg'
	},
	'x86_64-pc-windows-msvc': {
		url: `${BTBN}/ffmpeg-n9.0.2-39-g7adfbe40dc-win64-gpl-9.0.zip`,
		sha256: '693f3e224bc184b40524a62ca47526cfebaf2504414021b92fe75f677ad868b9',
		exe: 'ffmpeg.exe'
	},
	'aarch64-apple-darwin': {
		url: 'https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip',
		sha256: 'c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924',
		exe: 'ffmpeg'
	}
};

const here = dirname(fileURLToPath(import.meta.url));
const target = execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (\S+)/m)?.[1];
const build = target && BUILDS[target];
if (!build) {
	console.error(`No ffmpeg build is pinned for ${target ?? 'this machine'}.`);
	process.exit(1);
}
const outDir = join(here, '..', 'src-tauri', 'binaries');
const out = join(outDir, `pencilpilot-ffmpeg-${target}${target.includes('windows') ? '.exe' : ''}`);
if (existsSync(out)) {
	console.log(`${out} is already here.`);
	process.exit(0);
}

console.log(`Fetching ${build.url}`);
const res = await fetch(build.url, { redirect: 'follow' });
if (!res.ok) throw new Error(`${res.status} ${res.statusText}: ${build.url}`);
const bytes = Buffer.from(await res.arrayBuffer());
const sum = createHash('sha256').update(bytes).digest('hex');
if (sum !== build.sha256) throw new Error(`SHA-256 mismatch: got ${sum}, want ${build.sha256}`);

const work = mkdtempSync(join(tmpdir(), 'pp-ffmpeg-'));
try {
	const archive = join(work, build.url.split('/').pop());
	writeFileSync(archive, bytes);
	// (tar reads .tar.xz everywhere, and zips on Windows and macOS, where it's bsdtar)
	execFileSync('tar', ['-xf', archive, '-C', work], { stdio: 'inherit' });
	const found = (function find(dir) {
		for (const name of readdirSync(dir)) {
			const p = join(dir, name);
			if (statSync(p).isDirectory()) {
				const f = find(p);
				if (f) return f;
			} else if (name === build.exe) return p;
		}
		return null;
	})(work);
	if (!found) throw new Error(`no ${build.exe} in the archive`);
	mkdirSync(outDir, { recursive: true });
	copyFileSync(found, out);
	chmodSync(out, 0o755);
	console.log(`ffmpeg for ${target} → ${out}`);
} finally {
	rmSync(work, { recursive: true, force: true });
}
