/**
 * Updates from the website (pencilpilot.app/updates/latest.json, signed: the app installs
 * only what the release key signed). Looks shortly after the app starts and every few
 * hours while it runs, unless Settings says not to; a new version is offered in a corner
 * card (UpdatePrompt), never installed without asking. Installing goes through the open
 * project's "Save before…?" first: on Windows the installer closes the app.
 */
import { isTauri } from '@tauri-apps/api/core';
import type { Update } from '@tauri-apps/plugin-updater';
import { settings } from './settings.svelte';

export type UpdateStage =
	| { stage: 'none' }
	| { stage: 'offered'; version: string; notes: string }
	| { stage: 'downloading'; version: string; notes: string; got: number; total: number | null }
	| { stage: 'ready'; version: string; notes: string }
	| { stage: 'installing'; version: string }
	| { stage: 'failed'; version: string; notes: string; message: string };

/** what the corner card shows */
export const updates = $state<{ now: UpdateStage; hidden: boolean }>({
	now: { stage: 'none' },
	hidden: false
});

/** the first look, after the app has settled; then every few hours */
export const FIRST_CHECK_MS = 20_000;
export const CHECK_EVERY_MS = 6 * 3600_000;

let found: Update | null = null;
/** the open project's "Save before updating?" (true: go ahead) */
let leaveGuard: (() => Promise<boolean>) | null = null;

/** Offer a version found? Not one skipped, and only while nothing else is under way. */
export function shouldOffer(version: string, skipVersion: string, now: UpdateStage): boolean {
	if (version === skipVersion) return false;
	return now.stage === 'none' || now.stage === 'offered' || now.stage === 'failed';
}

/** "12.3 of 140 MB" */
export function progressText(got: number, total: number | null): string {
	const mb = (n: number) => (n / 1048576).toFixed(n < 10 * 1048576 ? 1 : 0);
	return total ? `${mb(got)} of ${mb(total)} MB` : `${mb(got)} MB`;
}

/**
 * The open project asks before an update closes the app. Returns the undo (the project
 * closing).
 */
export function guardUpdate(guard: () => Promise<boolean>): () => void {
	leaveGuard = guard;
	return () => {
		if (leaveGuard === guard) leaveGuard = null;
	};
}

/**
 * Look for a new version. `asked`: the person pressed "Check now" (a skipped version is
 * offered again, and the card shows even if it was put away).
 */
export async function checkForUpdate(asked = false): Promise<'new' | 'latest' | 'error'> {
	if (!isTauri()) return 'latest';
	try {
		const { check } = await import('@tauri-apps/plugin-updater');
		const u = await check({ timeout: 30_000 });
		if (!u) return 'latest';
		const skip = asked ? '' : settings.skipVersion;
		if (!shouldOffer(u.version, skip, updates.now)) return u.version === skip ? 'latest' : 'new';
		void found?.close().catch(() => {});
		found = u;
		updates.now = { stage: 'offered', version: u.version, notes: u.body?.trim() ?? '' };
		updates.hidden = false;
		if (asked && settings.skipVersion === u.version) settings.skipVersion = '';
		return 'new';
	} catch (e) {
		console.warn('update check failed', e);
		return 'error';
	}
}

/** Look now and then, as Settings says. Returns the stop. */
export function startChecking(): () => void {
	// a development build is never "behind" a release
	if (!isTauri() || import.meta.env.DEV) return () => {};
	const look = () => {
		if (settings.checkUpdates) void checkForUpdate();
	};
	const first = setTimeout(look, FIRST_CHECK_MS);
	const every = setInterval(look, CHECK_EVERY_MS);
	return () => {
		clearTimeout(first);
		clearInterval(every);
	};
}

/** Not now: the card goes until the next look finds it again (or Settings → Check now). */
export function later() {
	updates.hidden = true;
}

/** Never offer this version again (a newer one still is). */
export function skip() {
	const now = updates.now;
	if (now.stage === 'offered' || now.stage === 'failed') settings.skipVersion = now.version;
	updates.now = { stage: 'none' };
	updates.hidden = false;
}

/**
 * Update: download (if not yet), then the project's "Save before updating?", then install
 * and restart. Cancelling the question leaves the update ready for later.
 */
export async function install() {
	const u = found;
	const now = updates.now;
	if (!u || (now.stage !== 'offered' && now.stage !== 'ready' && now.stage !== 'failed')) return;
	const { version, notes } = now;
	try {
		if (now.stage !== 'ready') {
			updates.now = { stage: 'downloading', version, notes, got: 0, total: null };
			await u.download((e) => {
				const d = updates.now;
				if (d.stage !== 'downloading') return;
				if (e.event === 'Started') d.total = e.data.contentLength ?? null;
				else if (e.event === 'Progress') d.got += e.data.chunkLength;
			});
			updates.now = { stage: 'ready', version, notes };
		}
		if (leaveGuard && !(await leaveGuard())) return;
		updates.now = { stage: 'installing', version };
		await u.install();
		const { relaunch } = await import('@tauri-apps/plugin-process');
		await relaunch();
	} catch (e) {
		updates.now = {
			stage: 'failed',
			version,
			notes,
			message: e instanceof Error ? e.message : String(e)
		};
	}
}
