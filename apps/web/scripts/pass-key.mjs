// A new key pair for the app's Draw together passes (ADR-0013).
//
//   node apps/web/scripts/pass-key.mjs
//
// The private half goes in the site's environment (PASS_SIGNING_KEY, keep it secret); the
// public half is built into the app (PENCILPILOT_PASS_KEY when building it). With a seed as
// the argument, prints that seed's public half instead.
import { createPrivateKey, createPublicKey, randomBytes } from 'node:crypto';

const seed = process.argv[2] ? Buffer.from(process.argv[2].trim(), 'hex') : randomBytes(32);
if (seed.length !== 32) {
	console.error('A seed is 64 hex characters.');
	process.exit(1);
}
const key = createPrivateKey({
	key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]),
	format: 'der',
	type: 'pkcs8'
});
const spki = createPublicKey(key).export({ format: 'der', type: 'spki' });
if (!process.argv[2]) console.log(`PASS_SIGNING_KEY=${seed.toString('hex')}`);
console.log(`PENCILPILOT_PASS_KEY=${spki.subarray(spki.length - 32).toString('hex')}`);
