import { readFile } from 'node:fs/promises';
import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test';
import { APP, RECEIVER } from '../playwright.config';

const EMAIL = 'kris@example.com';
const PASSWORD = 'correct horse battery';

/** The source key, read from the setup screen and reused by later tests. */
let sourceKey = '';
/** The id of the "Charge failed" event, sent by the feed test and reused by the template test. */
let chargeEventId = '';

test.describe.configure({ mode: 'serial' });

async function sendEvent(request: APIRequestContext, body: Record<string, unknown>): Promise<string> {
	const response = await request.post(`${APP}/v1/events`, {
		headers: { authorization: `Bearer ${sourceKey}` },
		data: body
	});
	expect(response.status()).toBe(202);
	return (await response.json()).ids[0];
}

async function received(request: APIRequestContext): Promise<{ body: { title: string } }[]> {
	return (await request.get(`${RECEIVER}/requests`)).json();
}

async function signIn(page: Page) {
	await page.goto('/login');
	await page.getByLabel('Email').fill(EMAIL);
	await page.getByLabel('Password').fill(PASSWORD);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByRole('heading', { name: 'Feed' })).toBeVisible();
}

/** The rows of the notifications list that belong to one route. */
function notificationsOf(page: Page, route: string): Locator {
	return page.getByRole('list', { name: 'Notifications' }).getByRole('listitem').filter({ hasText: route });
}

/** WCAG contrast ratio between an element's text colour and the page background. */
async function contrast(page: Page, selector: string): Promise<number> {
	return page.evaluate((sel) => {
		const channel = (value: number) => {
			const c = value / 255;
			return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
		};
		const luminance = (color: string) => {
			const [r, g, b] = color.match(/[\d.]+/g)!.slice(0, 3).map(Number);
			return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
		};
		const text = luminance(getComputedStyle(document.querySelector(sel)!).color);
		const background = luminance(getComputedStyle(document.documentElement).backgroundColor);
		return (Math.max(text, background) + 0.05) / (Math.min(text, background) + 0.05);
	}, selector);
}

test('first-run setup ends with a message in the channel', async ({ page, request }) => {
	await page.goto('/');
	await expect(page).toHaveURL(/\/setup$/);
	await expect(page.getByRole('button', { name: 'Create source' })).toBeDisabled();

	await page.getByLabel('Email').fill(EMAIL);
	await page.getByLabel('Password').fill(PASSWORD);
	await page.getByRole('button', { name: 'Create account' }).click();

	await page.getByRole('button', { name: 'Create source' }).click();
	const curl = await page.getByTestId('curl').innerText();
	expect(curl).toContain(`${APP}/v1/events`);
	sourceKey = curl.match(/Bearer (\S+)"/)![1];
	await expect(page.getByText('Waiting for your event')).toBeVisible();
	await page.screenshot({ path: 'e2e-shots/setup-waiting.png', fullPage: true });

	await sendEvent(request, { topic: 'ramzes.hello', title: 'Hello from my service' });
	await expect(page.getByText('Event received.')).toBeVisible();

	await page.getByLabel('Where should messages go?').selectOption('webhook');
	await page.getByLabel('Name').fill('test receiver');
	await page.getByLabel('URL').fill(`${RECEIVER}/hook`);
	await page.getByRole('button', { name: 'Save connection' }).click();
	await page.getByRole('button', { name: 'Send test message' }).click();
	await expect(page.getByText('Test message delivered.')).toBeVisible();

	await page.getByRole('button', { name: 'Create route and deliver the test event' }).click();
	await expect(page.getByText('Your test event:')).toContainText('delivered');
	await page.screenshot({ path: 'e2e-shots/setup-done.png', fullPage: true });

	const titles = (await received(request)).map((r) => r.body.title);
	expect(titles).toEqual(['Test message from ramzes', 'Hello from my service']);

	await page.getByRole('link', { name: 'See what happened to it' }).click();
	await expect(page.getByRole('heading', { name: 'Hello from my service' })).toBeVisible();
	await expect(page.getByText('Matched route Everything')).toBeVisible();
	await expect(page.getByText('to test receiver')).toBeVisible();
	await expect(page.getByText('HTTP 200')).toBeVisible();
});

test('a new event appears in the feed without a reload and its journey shows the delivery', async ({ page, request }) => {
	await signIn(page);
	await expect(page.getByRole('link', { name: /Hello from my service/ })).toBeVisible();

	chargeEventId = await sendEvent(request, {
		topic: 'payments.charge.failed',
		title: 'Charge failed: card network timeout',
		severity: 'critical',
		body: '14 charges failed in the last 30 s',
		labels: { env: 'prod', service: 'payments' },
		payload: { order: 7, attempts: 3 }
	});
	const row = page.getByRole('link', { name: /Charge failed: card network timeout/ });
	await expect(row).toBeVisible();
	await expect(row).toContainText('delivered');

	await row.click();
	await expect(page.getByText('Matched route Everything')).toBeVisible();
	await expect(page.getByText('HTTP 200')).toBeVisible();
	await expect(page.getByText('"order": 7')).toBeVisible();
	await page.screenshot({ path: 'e2e-shots/journey-delivered.png', fullPage: true });
});

test('a delivery the service refuses shows its error and the next attempt', async ({ page, request }) => {
	await request.post(`${RECEIVER}/status/503`);
	await signIn(page);
	const id = await sendEvent(request, { topic: 'jobs.export', title: 'Nightly export stuck', severity: 'warning' });

	await page.goto(`/events/${id}`);
	await expect(page.getByText('HTTP 503: upstream down').first()).toBeVisible();
	await expect(page.getByText(/Next attempt in \d+ s/)).toBeVisible();
	await page.screenshot({ path: 'e2e-shots/journey-retrying.png', fullPage: true });
	await request.post(`${RECEIVER}/status/200`);
});

test('filters narrow the feed and live in the URL', async ({ page }) => {
	await signIn(page);
	await page.getByLabel('Minimum severity').selectOption('critical');
	await expect(page).toHaveURL(/min_severity=critical/);
	await expect(page.getByRole('link', { name: /Charge failed/ })).toBeVisible();
	await expect(page.getByRole('link', { name: /Hello from my service/ })).toHaveCount(0);

	await page.reload();
	await expect(page.getByLabel('Minimum severity')).toHaveValue('critical');
	await expect(page.getByRole('link', { name: /Hello from my service/ })).toHaveCount(0);
});

test('the light theme applies and stays readable', async ({ page }) => {
	await signIn(page);
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
	await page.screenshot({ path: 'e2e-shots/feed-dark.png', fullPage: true });
	const title = 'ul[aria-label="Events"] a span.truncate > span:first-child';
	expect(await contrast(page, title)).toBeGreaterThan(7);
	expect(await contrast(page, 'ul[aria-label="Events"] a time')).toBeGreaterThan(4.5);

	await page.getByRole('button', { name: 'Light theme' }).click();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
	expect(await contrast(page, title)).toBeGreaterThan(7);
	expect(await contrast(page, 'ul[aria-label="Events"] a time')).toBeGreaterThan(4.5);
	await page.screenshot({ path: 'e2e-shots/feed-light.png', fullPage: true });

	await page.reload();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
	await page.getByRole('button', { name: 'Dark theme' }).click();
});

test('the management pages load and a route can be created', async ({ page }) => {
	await signIn(page);

	await page.getByRole('link', { name: 'Routes' }).click();
	await expect(page.getByText('Every event')).toBeVisible();
	await page.getByRole('button', { name: 'New route' }).click();
	await page.getByLabel('Name').fill('Payment failures');
	await page.getByLabel('Topic').fill('payments.*');
	await page.getByLabel('Minimum severity').selectOption('error');
	await page.getByRole('button', { name: 'Create route' }).click();
	await expect(page.getByText('topic payments.*, severity error or higher')).toBeVisible();
	await page.screenshot({ path: 'e2e-shots/routes.png', fullPage: true });

	await page.getByRole('button', { name: 'Move Payment failures up' }).click();
	await expect(page.locator('ol > li').first()).toContainText('Payment failures');

	for (const [link, shot] of [
		['Overview', 'overview-first'],
		['Notifications', 'notifications-first'],
		['Silences', 'silences-first'],
		['Templates', 'templates'],
		['Connections', 'connections'],
		['Sources', 'sources'],
		['Failures', 'failures'],
		['Settings', 'settings']
	]) {
		await page.getByRole('link', { name: link, exact: true }).click();
		await expect(page.getByRole('heading', { name: link, exact: true })).toBeVisible();
		await page.waitForLoadState('networkidle');
		await page.screenshot({ path: `e2e-shots/${shot}.png`, fullPage: true });
	}
});

test('the route tester shows what an unsaved expression would match', async ({ page }) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	await page.getByRole('button', { name: 'New route' }).click();
	await page.getByLabel('Name').fill('Big orders');
	await page.getByLabel('Expression').fill('payload.amount > 100');

	// The sample event is an order of 250 on the topic orders.payment.failed.
	await page.getByLabel('Event', { exact: true }).selectOption({ label: 'An event I write here' });
	const results = page.getByRole('list', { name: 'Result for each route' }).getByRole('listitem');
	const result = (name: string) => results.filter({ hasText: name });
	await expect(result('Big orders')).toHaveAttribute('data-outcome', 'matched');
	await expect(result('Big orders')).toContainText('being edited');
	await expect(result('Big orders')).toContainText('to test receiver: Payment failed for order 1042');
	await expect(result('Payment failures')).toHaveAttribute('data-outcome', 'not_matched');
	await expect(result('Payment failures')).toContainText('not matched');
	await expect(result('Everything')).toHaveAttribute('data-outcome', 'matched');
	await page.screenshot({ path: 'e2e-shots/route-tester.png', fullPage: true });

	await page.getByLabel('Expression').fill('payload.amount > 1000');
	await expect(result('Big orders')).toHaveAttribute('data-outcome', 'not_matched');

	await page.getByLabel('Expression').fill('payload.amount >');
	await expect(page.getByRole('region', { name: 'Try an event' }).getByRole('alert')).toContainText('Big orders');

	// Trying a route does not save it.
	await page.getByRole('button', { name: 'Cancel' }).click();
	await expect(page.getByRole('list', { name: 'Routes' }).getByRole('listitem')).toHaveCount(2);
	await expect(results).toHaveCount(2);
});

test('a template preview shows the worded title and the request', async ({ page }) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Templates', exact: true }).click();
	await expect(page.getByText('No templates yet')).toBeVisible();
	await page.getByRole('button', { name: 'New template' }).click();
	await page.getByLabel('Name').fill('With environment');
	await page.getByLabel('Title').fill('[{{ labels.env }}] {{ title }}');
	await page.getByLabel('Body').fill('{{ payload.attempts }} attempts');
	await page.getByLabel('With the event').selectOption(chargeEventId);

	await expect(page.getByTestId('preview-title')).toHaveText('[prod] Charge failed: card network timeout');
	await expect(page.getByTestId('preview-body')).toHaveText('3 attempts');

	await page.getByLabel('As sent to').selectOption({ label: 'test receiver' });
	await expect(page.getByTestId('request')).toContainText(`POST ${RECEIVER}/hook`);
	await expect(page.getByTestId('request')).toContainText('[prod] Charge failed: card network timeout');
	await page.screenshot({ path: 'e2e-shots/template-preview.png', fullPage: true });

	await page.getByLabel('Title').fill('{{ title | nosuch }}');
	await expect(page.getByRole('region', { name: 'Preview' }).getByRole('alert')).toContainText('nosuch');
	await expect(page.getByTestId('preview-title')).toHaveText('Charge failed: card network timeout');

	await page.getByLabel('Title').fill('[{{ labels.env }}] {{ title }}');
	await page.getByRole('button', { name: 'Create template' }).click();
	await expect(page.getByText('Template created.')).toBeVisible();
	await expect(page.getByRole('listitem').filter({ hasText: 'With environment' })).toBeVisible();
});

test('a grouped route collects two events into one notification, which is then sent and acknowledged', async ({
	page,
	request
}) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	await page.getByRole('button', { name: 'New route' }).click();
	await page.getByLabel('Name').fill('Deploys grouped');
	await page.getByLabel('Topic').fill('deploys.*');
	await page.getByLabel('Collect events and send them as one message').check();
	await page.getByLabel('Wait before sending', { exact: true }).fill('2');
	await page.getByRole('button', { name: 'Create route' }).click();
	await expect(page.getByTestId('shaping-summary')).toHaveText('groups everything for 2 s');
	await page.screenshot({ path: 'e2e-shots/routes-shaped.png', fullPage: true });

	await page.getByRole('link', { name: 'Notifications', exact: true }).click();
	await expect(page.getByRole('link', { name: 'Waiting' })).toHaveAttribute('aria-current', 'page');
	const response = await request.post(`${APP}/v1/events`, {
		headers: { authorization: `Bearer ${sourceKey}` },
		data: [
			{ topic: 'deploys.started', title: 'Deploy of checkout started', labels: { env: 'prod', service: 'checkout' } },
			{ topic: 'deploys.finished', title: 'Deploy of checkout finished', labels: { env: 'prod', service: 'checkout' } }
		]
	});
	expect(response.status()).toBe(202);

	// It appears by itself while the group is still collecting.
	const grouped = notificationsOf(page, 'Deploys grouped');
	await expect(grouped).toContainText('Deploy of checkout started');
	await expect(grouped.getByTestId('event-count')).toHaveText('2 events');
	await expect(grouped).toContainText('collecting');

	// Two seconds later it has been sent, and it leaves this view without a reload.
	await expect(grouped).toHaveCount(0);
	await page.getByRole('link', { name: /^Sent/ }).click();
	await expect(page).toHaveURL(/view=sent/);
	await expect(grouped.getByTestId('event-count')).toHaveText('2 events');
	await expect(grouped.locator('[data-state="sent"]')).toBeVisible();
	await page.screenshot({ path: 'e2e-shots/notifications-sent.png', fullPage: true });
	// The route sent one message for the two events. (The route "Everything" sent each on its own.)
	await expect
		.poll(async () => (await received(request)).filter((r) => r.body.title === '2 events').length)
		.toBe(1);

	await grouped.getByRole('button', { name: /^Acknowledge/ }).click();
	await expect(page.getByText('Acknowledged.')).toBeVisible();
	await expect(grouped).toHaveCount(0);
	await page.getByRole('link', { name: 'Acknowledged', exact: true }).click();
	await expect(grouped).toContainText('acknowledged by kris@…');

	await grouped.getByRole('link', { name: 'Deploy of checkout started' }).click();
	await expect(page.getByRole('heading', { name: 'Deploy of checkout started' })).toBeVisible();
	const roles = page.getByTestId('event-role');
	await expect(roles).toHaveText(['first', 'grouped']);
	await expect(page.getByRole('region', { name: 'Deliveries' })).toContainText('HTTP 200');
	await page.screenshot({ path: 'e2e-shots/notification-detail.png', fullPage: true });

	// The second event's own page says how it relates to the notification.
	await page.getByRole('link', { name: 'Deploy of checkout finished' }).click();
	await expect(page.getByTestId('role').first()).toHaveText('Joined a group of 2 events.');
});

test('a silence is refused without conditions, suppresses a matching event and can be ended', async ({ page, request }) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Silences', exact: true }).click();
	await page.getByRole('button', { name: 'New silence' }).click();
	await page.getByRole('button', { name: 'Create silence' }).click();
	await expect(page.getByRole('alert')).toContainText('Add at least one condition');

	await page.getByLabel('Topic').fill('deploys.*');
	await page.getByRole('button', { name: '30 min' }).click();
	await page.getByLabel('Comment').fill('release freeze');
	await page.getByRole('button', { name: 'Create silence' }).click();
	await expect(page.getByText('Silence created.')).toBeVisible();
	const active = page.getByRole('region', { name: 'Active silences' });
	await expect(active).toContainText('topic deploys.*');
	await expect(active).toContainText('release freeze');
	await expect(active).toContainText('ends in 29 min');

	const id = await sendEvent(request, { topic: 'deploys.started', title: 'Deploy of billing started', labels: { env: 'prod' } });
	await page.goto('/notifications?view=suppressed');
	const suppressed = notificationsOf(page, 'Deploy of billing started');
	await expect(suppressed.first()).toContainText('not sent');
	await expect(suppressed.first()).toContainText('silenced: release freeze');
	await page.screenshot({ path: 'e2e-shots/notifications-suppressed.png', fullPage: true });
	const titles = (await received(request)).map((r) => r.body.title);
	expect(titles.filter((title) => title.includes('Deploy of billing started'))).toHaveLength(0);

	await page.goto(`/events/${id}`);
	await expect(page.getByText('Not sent').first()).toBeVisible();
	await expect(page.getByText('silenced: release freeze').first()).toBeVisible();

	await page.getByRole('link', { name: 'Silences', exact: true }).click();
	await active.getByRole('button', { name: 'End now' }).click();
	await expect(active).toContainText('No silence is active');
	await page.getByText('Ended (1)').click();
	await expect(page.locator('[data-kind="ended"]')).toContainText('topic deploys.*');
	await page.screenshot({ path: 'e2e-shots/silences.png', fullPage: true });
});

test('"Silence like this" opens the silence form filled in from the notification', async ({ page }) => {
	await signIn(page);
	await page.goto('/notifications?view=suppressed');
	await notificationsOf(page, 'Deploy of billing started')
		.first()
		.getByRole('button', { name: /^Silence events like/ })
		.click();
	await expect(page).toHaveURL(/\/silences\?topic=deploys\.started&label=env%3Dprod$/);
	await expect(page.getByLabel('Topic')).toHaveValue('deploys.started');
	await expect(page.getByLabel('Label key')).toHaveValue('env');
	await expect(page.getByLabel('Label value')).toHaveValue('prod');
	await expect(page.getByText('Silences: topic deploys.started, env = prod')).toBeVisible();
});

test('the overview counts what arrived and links to what needs attention', async ({ page }) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Overview', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
	const total = Number(await page.getByTestId('events-total').innerText());
	// The six events the tests above sent.
	expect(total).toBe(6);
	// One column for each of the last 24 hours and the current one; together they hold every event.
	await expect(page.getByTestId('bar')).toHaveCount(25);
	const perBar = await page.getByTestId('bar').evaluateAll((bars) => bars.map((bar) => Number(bar.getAttribute('data-events'))));
	expect(perBar.reduce((sum, events) => sum + events, 0)).toBe(total);
	await expect(page.getByTestId('chart-summary')).toContainText('Most in one hour:');
	await expect(page.getByRole('region', { name: 'Noisiest topics' })).toContainText('deploys.started');
	await expect(page.getByTestId('deliveries-delivered')).toContainText('delivered');
	await page.screenshot({ path: 'e2e-shots/overview.png', fullPage: true });

	await page.getByLabel('Period').selectOption({ label: 'Last 7 days' });
	await expect(page).toHaveURL(/hours=168/);
	await expect(page.getByTestId('bar')).toHaveCount(8);
	await expect(page.getByTestId('chart-summary')).toContainText('Most in one day');

	await page.getByTestId('unacknowledged-now').click();
	await expect(page).toHaveURL(/\/notifications\?view=sent$/);
	await expect(page.getByRole('link', { name: /^Sent/ })).toHaveAttribute('aria-current', 'page');
});

test('the template preview sends the destination to a connection that takes one', async ({ page }) => {
	await signIn(page);
	const created = await page.request.post('/api/connections', {
		headers: { 'x-ramzes-request': '1' },
		data: { kind: 'ntfy', name: 'phone push', settings: { server: RECEIVER, topic: 'ops' } }
	});
	expect(created.status()).toBe(201);

	await page.getByRole('link', { name: 'Templates', exact: true }).click();
	await page.getByRole('button', { name: 'Edit' }).click();
	// A webhook has no destination of its own, so there is nothing to fill in.
	await page.getByLabel('As sent to').selectOption({ label: 'test receiver' });
	await expect(page.getByTestId('request')).toContainText(`POST ${RECEIVER}/hook`);
	await expect(page.getByLabel('Topic')).toHaveCount(0);

	await page.getByLabel('As sent to').selectOption({ label: 'phone push' });
	await expect(page.getByTestId('request')).toContainText('ops');
	await page.getByLabel('Topic').fill('night-shift');
	await expect(page.getByTestId('request')).toContainText('night-shift');
	await expect(page.getByTestId('request')).not.toContainText('ops');
});

test('an exported configuration can be imported again, and the import rolled back', async ({ page }) => {
	await signIn(page);
	await page.getByRole('link', { name: 'Settings', exact: true }).click();
	const downloading = page.waitForEvent('download');
	await page.getByRole('button', { name: 'Download ramzes.yaml' }).click();
	const download = await downloading;
	expect(download.suggestedFilename()).toBe('ramzes.yaml');
	const yaml = await readFile(await download.path(), 'utf8');
	expect(yaml).toContain('Deploys grouped');

	// Change the configuration, so that importing the file has something to put back.
	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	const routes = page.getByRole('list', { name: 'Routes' }).getByRole('listitem');
	await routes.filter({ hasText: 'Deploys grouped' }).getByRole('button', { name: 'Delete' }).click();
	await page.getByRole('button', { name: 'Delete route' }).click();
	await expect(routes.filter({ hasText: 'Deploys grouped' })).toHaveCount(0);

	await page.getByRole('link', { name: 'Settings', exact: true }).click();
	await page.getByLabel('Or paste the YAML').fill(yaml);
	await page.getByRole('button', { name: 'Import configuration' }).click();
	await page.getByRole('button', { name: 'Replace configuration' }).click();
	await expect(page.getByText('Configuration imported.')).toBeVisible();

	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	await expect(routes.filter({ hasText: 'Deploys grouped' })).toContainText('groups everything for 2 s');
	await expect(routes.filter({ hasText: 'Payment failures' })).toContainText('topic payments.*, severity error or higher');

	// The newest entry of the history is the import; the one before it is the deletion.
	await page.getByRole('link', { name: 'Settings', exact: true }).click();
	const history = page.getByRole('region', { name: 'Configuration history' }).getByRole('listitem');
	await history.nth(1).getByRole('button', { name: 'Show changes' }).click();
	await expect(history.nth(1).getByTestId('config-diff')).toContainText('Deploys grouped');
	await history.nth(1).getByRole('button', { name: 'Roll back to this' }).click();
	await page.getByRole('button', { name: 'Roll back', exact: true }).click();
	await expect(page.getByText('Configuration rolled back.')).toBeVisible();

	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	await expect(routes.filter({ hasText: 'Payment failures' })).toBeVisible();
	await expect(routes.filter({ hasText: 'Deploys grouped' })).toHaveCount(0);
});

test('a connection that keeps failing is paused and can be resumed', async ({ page, request }) => {
	await request.post(`${RECEIVER}/status/503`);
	await signIn(page);
	for (let i = 1; i <= 5; i++) {
		await sendEvent(request, { topic: 'jobs.sync', title: `Sync ${i} failed`, severity: 'warning' });
	}
	const paused = page.getByTestId('paused');
	// The page does not follow deliveries live, so it is opened again until the fifth attempt has failed.
	await expect(async () => {
		await page.goto('/connections');
		await expect(paused).toContainText(/Paused until \d\d:\d\d after 5 failed\s+attempts in a row/, { timeout: 1000 });
	}).toPass();
	await page.screenshot({ path: 'e2e-shots/connection-paused.png', fullPage: true });

	await request.post(`${RECEIVER}/status/200`);
	await paused.getByRole('button', { name: 'Resume now' }).click();
	await expect(page.getByText('test receiver resumed.')).toBeVisible();
	await expect(paused).toHaveCount(0);
});

test('a route escalates along its chain until the notification is acknowledged', async ({ page, request }) => {
	await signIn(page);
	const headers = { 'x-ramzes-request': '1' };
	const connect = async (name: string): Promise<string> => {
		const created = await page.request.post('/api/connections', {
			headers,
			data: { kind: 'webhook', name, settings: { url: `${RECEIVER}/hook` } }
		});
		expect(created.status()).toBe(201);
		return (await created.json()).id;
	};
	await connect('ops lead');
	const leaving = await connect('leaving soon');

	await page.getByRole('link', { name: 'Routes', exact: true }).click();
	await page.getByRole('button', { name: 'New route' }).click();
	await page.getByLabel('Name').fill('Database paged');
	await page.getByLabel('Topic').fill('infra.*');
	await expect(page.getByText('The delay of a step counts from the step before it')).toBeVisible();

	await page.getByRole('button', { name: 'Add a step' }).click();
	await page.getByLabel('Step 1: delay').fill('10');
	await page.getByLabel('Step 1: Connection').selectOption({ label: 'leaving soon' });
	await page.getByRole('button', { name: 'Add another step' }).click();
	await page.getByLabel('Step 2: delay').fill('20');
	await page.getByLabel('Step 2: Connection').selectOption({ label: 'phone push' });
	await page.getByLabel('Step 2: Topic').fill('night-shift');
	await expect(page.getByTestId('since-first')).toHaveText([
		'10 min after the first message',
		'30 min after the first message'
	]);
	await page.screenshot({ path: 'e2e-shots/route-escalation.png', fullPage: true });

	// What the server refuses in a step is shown under the form.
	expect((await page.request.delete(`/api/connections/${leaving}`, { headers })).status()).toBe(204);
	await page.getByRole('button', { name: 'Create route' }).click();
	await expect(page.locator('form').getByRole('alert')).toContainText(leaving);

	await page.getByLabel('Step 1: Connection').selectOption({ label: 'ops lead' });
	await page.getByRole('button', { name: 'Create route' }).click();
	await expect(page.getByText('Route created.')).toBeVisible();
	await expect(page.getByTestId('chain-summary')).toHaveText(
		'then ops lead after 10 min, then phone push (night-shift) after 30 min'
	);

	// The chain survives the route being opened and saved again.
	const routes = page.getByRole('list', { name: 'Routes' }).getByRole('listitem');
	await routes.filter({ hasText: 'Database paged' }).getByRole('button', { name: 'Edit' }).click();
	await expect(page.getByLabel('Step 2: delay')).toHaveValue('20');
	await expect(page.getByLabel('Step 2: Topic')).toHaveValue('night-shift');
	await page.getByRole('button', { name: 'Save route' }).click();
	await expect(page.getByTestId('chain-summary')).toContainText('then ops lead after 10 min');

	await sendEvent(request, { topic: 'infra.db', title: 'Primary database is down', severity: 'critical' });
	await page.goto('/notifications?view=sent');
	const paged = notificationsOf(page, 'Database paged');
	await expect(paged).toContainText(/next step in 9 min/);
	await paged.getByRole('link', { name: 'Primary database is down' }).click();

	const chain = page.getByRole('region', { name: 'If nobody acknowledges' });
	const steps = chain.getByRole('list', { name: 'Escalation chain' }).getByRole('listitem');
	await expect(steps.nth(0)).toContainText(/First message\s+to test receiver\s+sent at \d\d:\d\d/);
	await expect(steps.nth(1)).toContainText('to ops lead, 10 min after the first message');
	await expect(steps.nth(2)).toContainText('to phone push (night-shift), 30 min after the first message');
	const states = chain.getByTestId('step-state');
	await expect(states).toHaveText([/^due in 9 min( \d+ s)?$/, 'not reached']);
	// The countdown runs by itself.
	const before = await states.nth(0).innerText();
	await expect(states.nth(0)).not.toHaveText(before);
	await page.screenshot({ path: 'e2e-shots/notification-escalating.png', fullPage: true });

	await page.getByRole('button', { name: /^Acknowledge/ }).click();
	await expect(page.getByText('Acknowledged.')).toBeVisible();
	await expect(chain.getByTestId('chain-stopped')).toHaveText(/^stopped: acknowledged by kris@… at \d\d:\d\d$/);
	await expect(states).toHaveText(['not reached', 'not reached']);
	await expect(steps).toHaveCount(4);
	await page.screenshot({ path: 'e2e-shots/notification-chain-stopped.png', fullPage: true });
});

test('the feed works on a phone-sized screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await signIn(page);
	await expect(page.getByRole('link', { name: /Charge failed/ })).toBeVisible();
	const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
	expect(overflow).toBeLessThanOrEqual(0);
	await page.screenshot({ path: 'e2e-shots/feed-phone.png', fullPage: true });
});

test('every page is reachable from the menu on a phone-sized screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await signIn(page);
	const nav = page.getByRole('navigation', { name: 'Main' });
	await expect(nav).toBeHidden();
	await page.getByRole('button', { name: 'Menu', exact: true }).click();
	for (const name of [
		'Overview',
		'Feed',
		'Notifications',
		'Failures',
		'Silences',
		'Routes',
		'Templates',
		'Connections',
		'Sources',
		'Settings'
	]) {
		await expect(nav.getByRole('link', { name, exact: true })).toBeInViewport();
	}
	await page.screenshot({ path: 'e2e-shots/menu-phone.png' });

	// Choosing a page folds the menu away again.
	await nav.getByRole('link', { name: 'Notifications', exact: true }).click();
	await expect(page.getByRole('heading', { name: 'Notifications' })).toBeVisible();
	await expect(nav).toBeHidden();
	await page.getByRole('link', { name: 'All', exact: true }).click();
	await expect(page.getByRole('button', { name: /^Acknowledge/ }).first()).toBeVisible();
	const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
	expect(overflow).toBeLessThanOrEqual(0);
	await page.screenshot({ path: 'e2e-shots/notifications-phone.png', fullPage: true });
});

test('theme and sign-out are reachable on a phone-sized screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await signIn(page);
	await page.getByRole('button', { name: 'Menu', exact: true }).click();
	await expect(page.getByRole('button', { name: 'Light theme' })).toBeVisible();
	await page.getByRole('button', { name: 'Sign out' }).click();
	await expect(page).toHaveURL(/\/login$/);
	await expect(page.getByRole('heading', { name: 'Sign in to ramzes' })).toBeVisible();
});
