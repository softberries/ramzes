import { expect, test, type APIRequestContext, type Page } from '@playwright/test';
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
	await expect(result('Big orders')).toContainText('not saved');
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

test('the feed works on a phone-sized screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await signIn(page);
	await expect(page.getByRole('link', { name: /Charge failed/ })).toBeVisible();
	const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
	expect(overflow).toBeLessThanOrEqual(0);
	await page.screenshot({ path: 'e2e-shots/feed-phone.png', fullPage: true });
});

test('theme and sign-out are reachable on a phone-sized screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await signIn(page);
	await expect(page.getByRole('button', { name: 'Light theme' })).toBeVisible();
	await page.getByRole('button', { name: 'Sign out' }).click();
	await expect(page).toHaveURL(/\/login$/);
	await expect(page.getByRole('heading', { name: 'Sign in to ramzes' })).toBeVisible();
});
