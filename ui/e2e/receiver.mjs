// A stand-in for a service that receives webhooks: records what it gets and answers as told.
//   POST /hook      -> answers with the current status (200 by default) and records the request
//   GET  /requests  -> the recorded requests as JSON
//   POST /status/N  -> makes /hook answer with status N from now on
import http from 'node:http';

const port = Number(process.argv[2] ?? 4791);
const requests = [];
let status = 200;

http
	.createServer((req, res) => {
		let body = '';
		req.on('data', (chunk) => (body += chunk));
		req.on('end', () => {
			if (req.method === 'GET' && req.url === '/requests') {
				res.writeHead(200, { 'content-type': 'application/json' });
				res.end(JSON.stringify(requests));
			} else if (req.method === 'POST' && req.url?.startsWith('/status/')) {
				status = Number(req.url.slice('/status/'.length));
				res.writeHead(204).end();
			} else if (req.method === 'POST' && req.url === '/hook') {
				requests.push({ headers: req.headers, body: JSON.parse(body) });
				res.writeHead(status, { 'content-type': 'text/plain' });
				res.end(status === 200 ? 'ok' : 'upstream down');
			} else {
				res.writeHead(200).end('receiver');
			}
		});
	})
	.listen(port, '127.0.0.1');
