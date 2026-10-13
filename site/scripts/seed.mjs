// Fills a fresh ramzes instance with varied demo data, for the screenshots in the documentation.
//   node scripts/seed.mjs <ramzes url> <receiver url> [events]
const APP = process.argv[2] ?? 'http://127.0.0.1:4795';
const RECEIVER = process.argv[3] ?? 'http://127.0.0.1:4796';
let cookie = '';
async function call(method, path, body, headers = {}) {
	const res = await fetch(APP + path, {
		method,
		headers: { 'content-type': 'application/json', 'x-ramzes-request': '1', cookie, ...headers },
		body: body === undefined ? undefined : JSON.stringify(body)
	});
	const set = res.headers.get('set-cookie');
	if (set) cookie = set.split(';')[0];
	const text = await res.text();
	if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text}`);
	return text ? JSON.parse(text) : null;
}
await call('POST', '/api/setup', { email: 'kris@example.com', password: 'correct horse battery' });
await call('POST', '/api/auth/login', { email: 'kris@example.com', password: 'correct horse battery' });
const sources = {};
for (const name of ['orders-api', 'billing', 'ci', 'infra-monitor']) sources[name] = (await call('POST', '/api/sources', { name })).key;
const conn = async (name, url) => (await call('POST', '/api/connections', { kind: 'webhook', name, settings: { url }, credentials: {} })).id;
const ops = await conn('#ops (Slack)', `${RECEIVER}/hook`);
const oncall = await conn('On-call pager', `${RECEIVER}/hook`);
const audit = await conn('Audit archive', `${RECEIVER}/hook`);
// Nothing answers here with success, so this connection fails, is paused and fills the failures page.
const broken = await conn('Legacy mailer', `${APP}/no-such-endpoint`);
const route = (body) => call('POST', '/api/routes', body);
await route({ name: 'Critical to on-call', match: { min_severity: 'critical' }, targets: [{ connection_id: oncall }], shaping: { dedup: { window_s: 3600 } }, escalation: [{ after_s: 600, targets: [{ connection_id: ops }] }] });
await route({ name: 'Orders and billing errors', match: { topic: 'orders.*', min_severity: 'warning' }, targets: [{ connection_id: ops }], shaping: { group: { by: ['service'], wait_s: 3600 } } });
await route({ name: 'Billing to mailer', match: { topic: 'billing.*' }, targets: [{ connection_id: broken }] });
await route({ name: 'CI results', match: { topic: 'ci.*' }, targets: [{ connection_id: ops }] });
await route({ name: 'Everything to audit', match: {}, targets: [{ connection_id: audit }] });
await call('POST', '/api/templates', { name: 'Short for chat', title: '[{{ labels.env }}] {{ title }}', body: '{{ topic }} from {{ labels.service }}' });
await call('POST', '/api/sources', { name: 'shop repository', kind: 'github' });
await call('PUT', '/api/settings', { retention_days: 30, retention_overrides: {}, public_url: APP });
await call('POST', '/api/silences', { match: { topic: 'infra.disk.*' }, starts_at: Date.now() - 60000, ends_at: Date.now() + 7200000, comment: 'Disk migration on db-3' }).catch((e) => console.log(String(e)));
const pick = (xs) => xs[Math.floor(Math.random() * xs.length)];
const kinds = [
	['orders-api', 'orders.failed', ['error', 'error', 'warning'], (i) => `Order ${1000 + i} failed: card declined`],
	['orders-api', 'orders.slow', ['warning', 'info'], (i) => `Checkout p95 above 2s (${i % 7 + 2}.1s)`],
	['billing', 'billing.invoice.failed', ['error'], (i) => `Invoice INV-${2000 + i} could not be issued`],
	['billing', 'billing.payout.sent', ['info'], (i) => `Payout batch ${i} sent`],
	['ci', 'ci.build', ['info', 'info', 'error'], (i) => `shop/web build #${300 + i}`],
	['infra-monitor', 'infra.db.down', ['critical'], () => 'Primary database unreachable'],
	['infra-monitor', 'infra.disk.full', ['warning', 'error'], (i) => `Disk above 9${i % 10}% on db-${i % 4}`],
	['infra-monitor', 'infra.cert.expiring', ['warning'], (i) => `Certificate for api-${i % 3}.example.com expires in ${i % 14 + 1} days`],
	['infra-monitor', 'infra.heartbeat', ['debug'], (i) => `Heartbeat ${i}`]
];
const weights = [6, 3, 2, 2, 5, 1, 2, 1, 4];
const bag = kinds.flatMap((k, i) => Array(weights[i]).fill(k));
const N = Number(process.argv[4] ?? 420);
for (let i = 0; i < N; i++) {
	const [source, topic, sevs, title] = pick(bag);
	const body = { topic, title: title(i), severity: pick(sevs), labels: { env: pick(['prod', 'prod', 'staging']), service: topic.split('.')[0], region: pick(['eu-west', 'us-east']) }, payload: { n: i } };
	if (topic === 'infra.db.down') body.dedup_key = 'db-primary';
	await call('POST', '/v1/events', body, { authorization: `Bearer ${sources[source]}`, cookie: '' });
}
console.log('seeded', N);
