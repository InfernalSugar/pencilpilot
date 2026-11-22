/**
 * Session passes for the app's "Draw together" (ADR-0013, "Sign-in and passes"): a short
 * statement {account id, display name, the computer's member id, issued, expires}, signed
 * with the site's ed25519 key. Members' apps check each other's passes with the site's
 * public key, built into the app. No email ever goes in a pass.
 *
 * The signature covers "pencilpilot-pass v1\n" and then the body, byte for byte (the app
 * checks exactly that: crates/share/src/pass.rs).
 */
import { createPrivateKey, createPublicKey, sign, verify, type KeyObject } from 'node:crypto';
import { cleanName } from './lobby/ticket';

/**
 * The development key (public knowledge: development only). Debug builds of the app trust
 * it, so `pnpm --filter @pencilpilot/web dev` works with `pnpm app` out of the box.
 */
export const DEV_PASS_SEED = '62e1111add10abf5114b66e2caee869c95d97cf13b95d154524efcfee558cbf6';

/** How long a pass lasts. The app asks for a new one when it's near the end. */
export const PASS_HOURS = 24;

const CONTEXT = 'pencilpilot-pass v1\n';
// PKCS#8 wrapping of a raw 32-byte ed25519 private key
const PKCS8_PREFIX = Buffer.from('302e020100300506032b657004220420', 'hex');

export interface PassInfo {
	/** the account's id */
	user: string;
	/** the account's display name */
	name: string;
	/** the computer's member id (64 hex characters) */
	endpoint: string;
	/** seconds since 1970 */
	issued: number;
	expires: number;
}

export interface Pass {
	body: string;
	/** hex */
	sig: string;
}

/** The signing key from its 32-byte seed (64 hex characters), or null if that isn't one. */
export function keyFromSeed(hex: string): KeyObject | null {
	const seed = hex.trim().toLowerCase();
	if (!/^[0-9a-f]{64}$/.test(seed)) return null;
	return createPrivateKey({
		key: Buffer.concat([PKCS8_PREFIX, Buffer.from(seed, 'hex')]),
		format: 'der',
		type: 'pkcs8'
	});
}

/**
 * The site's signing key: `PASS_SIGNING_KEY`, else (development only) the development key.
 * Null when the site has none: passes can't be issued.
 */
export function passKey(setting: string | undefined, dev: boolean): KeyObject | null {
	if (setting) return keyFromSeed(setting);
	return dev ? keyFromSeed(DEV_PASS_SEED) : null;
}

/** The public half as the app has it built in (64 hex characters). */
export function publicKeyHex(key: KeyObject): string {
	const spki = createPublicKey(key).export({ format: 'der', type: 'spki' });
	return Buffer.from(spki.subarray(spki.length - 32)).toString('hex');
}

/** A computer's member id: an ed25519 public key in hex. */
export const validEndpoint = (s: unknown): s is string =>
	typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);

/** A pass for `who` on the computer `endpoint`, good for PASS_HOURS from `now` (ms). */
export function issuePass(
	key: KeyObject,
	who: { user: string; name: string; endpoint: string },
	now = Date.now()
): Pass {
	const issued = Math.floor(now / 1000);
	const body = JSON.stringify({
		v: 1,
		user: who.user,
		name: cleanName(who.name) ?? 'Someone',
		endpoint: who.endpoint,
		issued,
		expires: issued + PASS_HOURS * 3600
	});
	const sig = sign(null, Buffer.from(CONTEXT + body), key);
	return { body, sig: sig.toString('hex') };
}

/** What a pass says, if `publicHex` signed it and it's still good at `now` (ms). */
export function readPass(publicHex: string, pass: Pass, now = Date.now()): PassInfo | null {
	try {
		const pub = createPublicKey({
			key: Buffer.concat([
				Buffer.from('302a300506032b6570032100', 'hex'),
				Buffer.from(publicHex, 'hex')
			]),
			format: 'der',
			type: 'spki'
		});
		if (!verify(null, Buffer.from(CONTEXT + pass.body), pub, Buffer.from(pass.sig, 'hex')))
			return null;
		const b = JSON.parse(pass.body);
		if (b.v !== 1 || Math.floor(now / 1000) >= b.expires) return null;
		return {
			user: b.user,
			name: b.name,
			endpoint: b.endpoint,
			issued: b.issued,
			expires: b.expires
		};
	} catch {
		return null;
	}
}
