// Takes the screenshots the documentation uses, in both themes, from the demo instance that
// scripts/demo.sh starts. Writes src/assets/screens/<name>-<theme>.webp.
//   node scripts/capture.mjs [name…]      all shots, or only the named ones
import { mkdirSync } from 'node:fs';
import { chromium } from 'playwright';
import sharp from 'sharp';

const APP = process.env.RAMZES_URL ?? 'http://127.0.0.1:4795';
const OUT = new URL('../src/assets/screens/', import.meta.url).pathname;
const VIEW = { width: 1360, height: 860 };
const only = process.argv.slice(2);

/** `path` may use {event} and {chained}; `prepare` brings the page to the state shown. */
const SHOTS = [
	{ name: 'overview', path: '/overview', full: true },
	{ name: 'feed', path: '/' },
	{ name: 'event', path: '/events/{event}', full: true },
	{ name: 'notifications', path: '/notifications?view=sent' },
	{ name: 'notification', path: '/notifications/{chained}', full: true },
	{ name: 'failures', path: '/failures' },
	{ name: 'silences', path: '/silences' },
	{ name: 'routes', path: '/routes' },
	{
		name: 'route-editor',
		path: '/routes',
		full: true,
		prepare: (page) => page.getByRole('button', { name: 'Edit' }).first().click()
	},
	{ name: 'templates', path: '/templates' },
	{ name: 'connections', path: '/connections', full: true },
	{ name: 'sources', path: '/sources' },
	{ name: 'settings', path: '/settings', full: true },
	{ name: 'login', path: '/login', signedOut: true }
];

mkdirSync(OUT, { recursive: true });
const browser = await chromium.launch();

async function session(theme, signedIn) {
	const context = await browser.newContext({ viewport: VIEW, deviceScaleFactor: 2 });
	await context.addInitScript((t) => localStorage.setItem('ramzes-theme', t), theme);
	const page = await context.newPage();
	if (signedIn) {
		await page.goto(`${APP}/login`);
		await page.getByLabel('Email').fill('kris@example.com');
		await page.getByLabel('Password').fill('correct horse battery');
		await page.getByRole('button', { name: 'Sign in' }).click();
		await page.waitForURL(`${APP}/`);
	}
	return { context, page };
}

/** Ids of the records the detail pages show. */
async function ids(page) {
	return page.evaluate(async () => {
		const get = async (path) => (await fetch(path)).json();
		const delivered = await get('/api/events?outcome=delivered&min_severity=error&limit=1');
		const all = await get('/api/notifications?limit=200');
		// The route with an escalation chain, so the page shows how far the chain has come.
		const chained = all.notifications.find((n) => n.route_name === 'Critical to on-call') ?? all.notifications[0];
		return { event: delivered.events[0].id, chained: chained.id };
	});
}

for (const theme of ['dark', 'light']) {
	const signedIn = await session(theme, true);
	const signedOut = await session(theme, false);
	const known = await ids(signedIn.page);
	for (const shot of SHOTS) {
		if (only.length > 0 && !only.includes(shot.name)) continue;
		const { page } = shot.signedOut ? signedOut : signedIn;
		await page.goto(APP + shot.path.replace(/\{(\w+)\}/g, (_, key) => known[key]));
		await page.waitForLoadState('networkidle');
		await shot.prepare?.(page);
		// Arrival highlights and theme transitions have settled.
		await page.waitForTimeout(700);
		const png = await page.screenshot({ fullPage: shot.full ?? false });
		await sharp(png).webp({ quality: 86 }).toFile(`${OUT}${shot.name}-${theme}.webp`);
		console.log(`${shot.name}-${theme}`);
	}
	await signedIn.context.close();
	await signedOut.context.close();
}
await browser.close();
