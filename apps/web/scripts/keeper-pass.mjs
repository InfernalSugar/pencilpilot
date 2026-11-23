// A sign-in pass for the cloud's keeper (ADR-0014): the website's signed word that the
// keeper's computer key belongs to the cloud ("pp:cloud"), so apps let it into sessions.
//
//   node apps/web/scripts/keeper-pass.mjs <PASS_SIGNING_KEY> <keeper id> [days] > pass.json
//
// <PASS_SIGNING_KEY>: the site's signing seed (64 hex characters, as in its environment).
// <keeper id>: what `pencilpilot-keeper --id` prints. [days]: how long it lasts (365).
// The keeper warns in its log a month before it runs out: make a new one then.
import { createPrivateKey, sign } from 'node:crypto';

const [seedHex, endpoint, daysArg] = process.argv.slice(2);
const bad = (why) => {
	console.error(why);
	console.error('usage: keeper-pass.mjs <PASS_SIGNING_KEY> <keeper id> [days]');
	process.exit(1);
};
if (!/^[0-9a-f]{64}$/i.test(seedHex ?? '')) bad('The signing key is 64 hex characters.');
if (!/^[0-9a-f]{64}$/.test(endpoint ?? '')) bad('The keeper id is 64 hex characters.');
const days = daysArg ? Number(daysArg) : 365;
if (!Number.isInteger(days) || days < 1 || days > 3650) bad('Days: 1 to 3650.');

const key = createPrivateKey({
	key: Buffer.concat([
		Buffer.from('302e020100300506032b657004220420', 'hex'),
		Buffer.from(seedHex, 'hex')
	]),
	format: 'der',
	type: 'pkcs8'
});
const issued = Math.floor(Date.now() / 1000);
// the same statement as the site's passes (src/lib/server/pass.ts), for the cloud's account
const body = JSON.stringify({
	v: 1,
	user: 'pp:cloud',
	name: 'Cloud',
	endpoint,
	issued,
	expires: issued + days * 86_400
});
const sig = sign(null, Buffer.from('pencilpilot-pass v1\n' + body), key).toString('hex');
console.log(JSON.stringify({ body, sig }));
