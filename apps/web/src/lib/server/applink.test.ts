import { describe, expect, it } from 'vitest';
import { generateKeyPairSync } from 'node:crypto';
import { openDb } from './db';
import { createPasswordUser, deleteUser } from './auth';
import {
	appLabel,
	appUser,
	bearer,
	challengeOf,
	createLinkCode,
	exchangeCode,
	listLinks,
	loopbackRedirect,
	revokeToken,
	unlink,
	validChallenge
} from './applink';
import {
	DEV_PASS_SEED,
	PASS_HOURS,
	issuePass,
	keyFromSeed,
	passKey,
	publicKeyHex,
	readPass,
	validEndpoint
} from './pass';

const ENDPOINT = 'ab'.repeat(32);
const VERIFIER = 'a-verifier-only-the-app-knows-0123456789abcdef';
const REDIRECT = 'http://127.0.0.1:53111/linked';

describe('passes for drawing together', () => {
	it('signs a pass the app can check, with the name and no email', () => {
		const key = keyFromSeed(DEV_PASS_SEED)!;
		const pub = publicKeyHex(key);
		// the app's debug builds have this public key built in (src-tauri/src/account.rs)
		expect(pub).toBe('3b43523f310add603b4687893fafc8f094b5772e6a73d614a49b1a845c0045d0');
		const now = 1_700_000_000_000;
		const pass = issuePass(key, { user: 'u_ana', name: '  Ana\u0007 ', endpoint: ENDPOINT }, now);
		expect(pass.body).toBe(
			JSON.stringify({
				v: 1,
				user: 'u_ana',
				name: 'Ana',
				endpoint: ENDPOINT,
				issued: 1_700_000_000,
				expires: 1_700_000_000 + PASS_HOURS * 3600
			})
		);
		expect(pass.body).not.toContain('@');
		expect(readPass(pub, pass, now)?.name).toBe('Ana');
	});

	it('refuses a changed, forged or expired pass', () => {
		const key = keyFromSeed(DEV_PASS_SEED)!;
		const pub = publicKeyHex(key);
		const now = Date.now();
		const pass = issuePass(key, { user: 'u_ana', name: 'Ana', endpoint: ENDPOINT }, now);
		expect(readPass(pub, { ...pass, body: pass.body.replace('Ana', 'Bob') }, now)).toBeNull();
		const other = generateKeyPairSync('ed25519').privateKey;
		const forged = issuePass(other, { user: 'u_ana', name: 'Ana', endpoint: ENDPOINT }, now);
		expect(readPass(pub, forged, now)).toBeNull();
		expect(readPass(pub, pass, now + PASS_HOURS * 3600_000)).toBeNull();
	});

	it('uses the configured key, the development key only in development', () => {
		const mine = '11'.repeat(32);
		expect(publicKeyHex(passKey(mine, false)!)).toBe(publicKeyHex(keyFromSeed(mine)!));
		expect(passKey(undefined, true)).not.toBeNull();
		expect(passKey(undefined, false)).toBeNull();
		expect(keyFromSeed('not hex')).toBeNull();
		expect(validEndpoint(ENDPOINT)).toBe(true);
		expect(validEndpoint('AB'.repeat(32))).toBe(false);
		expect(validEndpoint('ab')).toBe(false);
	});
});

describe('linking the app', () => {
	async function setup() {
		const db = openDb(':memory:');
		const user = await createPasswordUser(db, 'ana@example.com', 'long enough', 'Ana');
		const code = createLinkCode(db, user, {
			challenge: challengeOf(VERIFIER),
			redirect: REDIRECT,
			label: 'PencilPilot on Linux'
		});
		return { db, user, code };
	}

	it('only sends codes back to the app on this computer', () => {
		expect(loopbackRedirect('http://127.0.0.1:53111/linked')).toBe(REDIRECT);
		expect(loopbackRedirect('http://[::1]:53111/linked')).toBe('http://[::1]:53111/linked');
		expect(loopbackRedirect('https://evil.example/linked')).toBeNull();
		expect(loopbackRedirect('http://localhost.evil.example:80/')).toBeNull();
		expect(loopbackRedirect('http://127.0.0.1/linked')).toBeNull(); // no port
		expect(loopbackRedirect('http://127.0.0.1:5/x?code=1')).toBeNull();
		expect(loopbackRedirect('http://me@127.0.0.1:5/x')).toBeNull();
		expect(loopbackRedirect(42)).toBeNull();
		expect(validChallenge(challengeOf(VERIFIER))).toBe(true);
		expect(validChallenge('short')).toBe(false);
		expect(appLabel(' PencilPilot \n on  Linux ')).toBe('PencilPilot on Linux');
		expect(appLabel(undefined)).toBe('PencilPilot app');
	});

	it('trades a code once for a token that only the app has', async () => {
		const { db, user, code } = await setup();
		const got = exchangeCode(db, { code, verifier: VERIFIER, redirect: REDIRECT })!;
		expect(got.user).toEqual({ id: user, name: 'Ana' });
		expect(got.token).toMatch(/^ppa_/);
		// one time only
		expect(exchangeCode(db, { code, verifier: VERIFIER, redirect: REDIRECT })).toBeNull();
		// the token works; only its hash is kept
		expect(appUser(db, got.token)?.user.id).toBe(user);
		const rows = db.prepare('SELECT token_hash FROM app_links').all() as { token_hash: string }[];
		expect(rows.map((r) => r.token_hash)).not.toContain(got.token);
		expect(bearer(`Bearer ${got.token}`)).toBe(got.token);
		expect(bearer('Basic abc')).toBeNull();
	});

	it('refuses the wrong secret or address, and uses the code up', async () => {
		const { db, code } = await setup();
		const other = 'someone-else-s-verifier-0123456789abcdefghijk';
		expect(exchangeCode(db, { code, verifier: other, redirect: REDIRECT })).toBeNull();
		// a wrong try spends it: the right one can't follow
		expect(exchangeCode(db, { code, verifier: VERIFIER, redirect: REDIRECT })).toBeNull();
		const again = await setup();
		expect(
			exchangeCode(again.db, {
				code: again.code,
				verifier: VERIFIER,
				redirect: 'http://127.0.0.1:1/linked'
			})
		).toBeNull();
		expect(
			exchangeCode(again.db, { code: 'nope', verifier: VERIFIER, redirect: REDIRECT })
		).toBeNull();
		expect(exchangeCode(again.db, { code: 5, verifier: VERIFIER, redirect: REDIRECT })).toBeNull();
	});

	it('codes expire after five minutes', async () => {
		const { db, code } = await setup();
		const later = Date.now() + 5 * 60_000 + 1;
		expect(exchangeCode(db, { code, verifier: VERIFIER, redirect: REDIRECT }, later)).toBeNull();
	});

	it('lists linked apps and unlinks them; signing out or deleting the account ends them', async () => {
		const { db, user, code } = await setup();
		const { token } = exchangeCode(db, { code, verifier: VERIFIER, redirect: REDIRECT })!;
		const [app] = listLinks(db, user);
		expect(app.label).toBe('PencilPilot on Linux');
		expect(app.last_used_at).toBeNull();
		appUser(db, token, 1234);
		expect(listLinks(db, user)[0].last_used_at).toBe(1234);
		expect(unlink(db, 'u_someone_else', app.id)).toBe(false);
		expect(unlink(db, user, app.id)).toBe(true);
		expect(appUser(db, token)).toBeNull();

		const second = createLinkCode(db, user, {
			challenge: challengeOf(VERIFIER),
			redirect: REDIRECT,
			label: 'x'
		});
		const t2 = exchangeCode(db, { code: second, verifier: VERIFIER, redirect: REDIRECT })!.token;
		expect(revokeToken(db, t2)).toBe(true);
		expect(appUser(db, t2)).toBeNull();

		const third = createLinkCode(db, user, {
			challenge: challengeOf(VERIFIER),
			redirect: REDIRECT,
			label: 'x'
		});
		const t3 = exchangeCode(db, { code: third, verifier: VERIFIER, redirect: REDIRECT })!.token;
		deleteUser(db, user);
		expect(appUser(db, t3)).toBeNull();
	});
});
