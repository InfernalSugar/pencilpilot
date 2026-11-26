#!/usr/bin/env node
// Set the app's version everywhere it's written, before a release:
//   node apps/studio/scripts/set-version.mjs 0.2.0
// tauri.conf.json (what the installers and the updater go by) and Cargo.toml (what the app
// reports about itself). Then release it with the tag v0.2.0 (the Release workflow checks).
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const version = process.argv[2]?.replace(/^v/, '');
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
	console.error('usage: set-version.mjs <version, e.g. 0.2.0 or 0.2.0-preview.1>');
	process.exit(2);
}
const tauri = join(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri');

const confPath = join(tauri, 'tauri.conf.json');
const conf = readFileSync(confPath, 'utf8');
writeFileSync(confPath, conf.replace(/^(\t"version": )"[^"]*"/m, `$1"${version}"`));

const cargoPath = join(tauri, 'Cargo.toml');
const cargo = readFileSync(cargoPath, 'utf8');
writeFileSync(cargoPath, cargo.replace(/^version = "[^"]*"/m, `version = "${version}"`));

console.log(`PencilPilot is now ${version}: commit, build, then release it as v${version}`);
