// Stand-in for the ref-service's NSFW guard in e2e runs (CI has no model).
// Drawings containing the bytes "NSFWTEST" are refused; everything else passes.
import { createServer } from 'node:http';

createServer((req, res) => {
	if (req.url === '/api/health') return res.end('ok');
	if (req.method === 'POST' && req.url === '/api/moderate') {
		const chunks = [];
		req.on('data', (c) => chunks.push(c));
		req.on('end', () => {
			const unsafe = Buffer.concat(chunks).includes('NSFWTEST');
			res.setHeader('content-type', 'application/json');
			res.end(JSON.stringify(unsafe ? { safe: false, reason: 'nsfw' } : { safe: true }));
		});
		return;
	}
	res.statusCode = 404;
	res.end();
}).listen(8799, '127.0.0.1');
