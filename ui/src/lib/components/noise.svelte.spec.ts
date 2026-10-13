import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { NotificationRow, NotificationState as State, Shaping } from '#lib/api/types.ts';

const { apiMock, gotoMock } = vi.hoisted(() => ({ apiMock: vi.fn(), gotoMock: vi.fn() }));
vi.mock('#lib/api/client.ts', async (original) => ({
	...(await original<typeof import('#lib/api/client.ts')>()),
	api: apiMock
}));
vi.mock('$app/navigation', () => ({ goto: gotoMock }));

import { emptyBucket } from '#lib/overview.ts';
import EventChart from './EventChart.svelte';
import NotificationActions from './NotificationActions.svelte';
import NotificationState from './NotificationState.svelte';
import ShapingEditorHarness from './ShapingEditorHarness.svelte';

const NOW = new Date(2026, 9, 13, 12, 30, 0).getTime();
const HOUR = 3_600_000;

function notification(state: State, extra: Partial<NotificationRow> = {}): NotificationRow {
	return {
		id: 'n1',
		route_id: 'r1',
		route_name: 'Payments',
		connection_id: 'c1',
		connection_name: 'ops',
		state,
		title: 'Charge failed',
		severity: 'error',
		event_count: 1,
		sent_count: 1,
		created_at: NOW - 600_000,
		updated_at: NOW - 180_000,
		...extra
	};
}

beforeEach(() => {
	apiMock.mockReset();
	gotoMock.mockReset();
});

describe('ShapingEditor', () => {
	/** Renders the editor and returns a reader for the policy it currently holds. */
	function editing(initial?: Shaping) {
		render(ShapingEditorHarness, { initial });
		return async () => JSON.parse((await page.getByTestId('shaping').element()).textContent ?? '{}') as Shaping;
	}

	it('starts as an empty policy with every part folded away', async () => {
		const shaping = editing();
		expect(await shaping()).toEqual({});
		await expect.element(page.getByLabelText('Labels to group by')).not.toBeInTheDocument();
		await expect.element(page.getByLabelText('Time zone')).not.toBeInTheDocument();
		await expect.element(page.getByLabelText('Tell the target when it is resolved')).toBeChecked();
	});

	it('reveals the fields of grouping and builds the policy from them', async () => {
		const shaping = editing();
		await page.getByLabelText('Collect events and send them as one message').click();
		await page.getByLabelText('Labels to group by').fill('service, env');
		await page.getByLabelText('Wait before sending', { exact: true }).fill('2');
		await page.getByLabelText('Wait before sending: unit').selectOptions('min');
		expect(await shaping()).toEqual({ group: { by: ['service', 'env'], wait_s: 120 } });
		await expect.element(page.getByTestId('summary')).toHaveTextContent('groups by service and env for 2 min');
	});

	it('builds quiet hours with chosen days and a severity that is sent anyway', async () => {
		const shaping = editing();
		await page.getByLabelText('Hold messages during quiet hours and send one digest after').click();
		await expect.element(page.getByLabelText('Time zone')).toHaveValue('Europe/Warsaw');
		await expect.element(page.getByText('None chosen: every day.')).toBeVisible();
		await page.getByRole('button', { name: 'Sun' }).click();
		await page.getByRole('button', { name: 'Sat' }).click();
		await expect.element(page.getByRole('button', { name: 'Sat' })).toHaveAttribute('aria-pressed', 'true');
		await page.getByLabelText('Send anyway at or above').selectOptions('critical');
		expect(await shaping()).toEqual({
			quiet_hours: { tz: 'Europe/Warsaw', start: '22:00', end: '07:00', days: ['sat', 'sun'], bypass_severity: 'critical' }
		});
		await page.getByRole('button', { name: 'Sat' }).click();
		expect((await shaping()).quiet_hours?.days).toEqual(['sun']);
	});

	it('shows a stored policy and drops a part when it is switched off', async () => {
		const shaping = editing({ dedup: { window_s: 600, key: 'labels.order' }, rate_limit: { max: 5, window_s: 300 } });
		await expect.element(page.getByLabelText('For this long after the first one', { exact: true })).toHaveValue(10);
		await expect.element(page.getByLabelText('What makes two events the same')).toHaveValue('labels.order');
		await expect.element(page.getByLabelText('At most this many')).toHaveValue(5);
		await page.getByLabelText('Limit how many messages are sent').click();
		expect(await shaping()).toEqual({ dedup: { window_s: 600, key: 'labels.order' } });
	});

	it('records that resolutions are not announced', async () => {
		const shaping = editing();
		await page.getByLabelText('Tell the target when it is resolved').click();
		expect(await shaping()).toEqual({ notify_resolved: false });
	});
});

describe('EventChart', () => {
	const start = new Date(2026, 9, 13, 9, 0, 0).getTime();
	const buckets = [3, 0, 9, 1].map((events, index) => ({ ...emptyBucket(start + index * HOUR), events, error: events }));

	it('draws one column for each bucket, the empty ones included, and names the peak', async () => {
		render(EventChart, { buckets, bucketMs: HOUR, now: NOW });
		await expect.element(page.getByTestId('chart-summary')).toHaveTextContent('Most in one hour: 9 events, 11:00');
		const bars = page.getByTestId('bar');
		expect(bars.elements().map((bar) => bar.getAttribute('data-events'))).toEqual(['3', '0', '9', '1']);
		expect(bars.elements().map((bar) => bar.querySelector('.bar') !== null)).toEqual([true, false, true, true]);
		// The first and the last bucket are named under the bars.
		await expect.element(page.getByText('09:00', { exact: true }).first()).toBeInTheDocument();
		await expect.element(page.getByText('12:00', { exact: true }).first()).toBeInTheDocument();
	});

	it('gives the same numbers as a table', async () => {
		render(EventChart, { buckets, bucketMs: HOUR, now: NOW });
		const rows = page.getByRole('row');
		// Start, total, then one number for each severity, most severe first.
		await expect.element(rows.nth(3)).toHaveTextContent('11:00909000');
		expect(rows.elements().length).toBe(5);
	});

	it('says so when no bucket has events', async () => {
		render(EventChart, { buckets: buckets.map((b) => emptyBucket(b.start)), bucketMs: HOUR, now: NOW });
		await expect.element(page.getByTestId('chart-summary')).toHaveTextContent('No events in any hour');
	});
});

describe('NotificationState', () => {
	it('shows the state, the time that matters and the reason', async () => {
		const held = notification('held', {
			reason: 'rate limit: 5 per 300 s already sent',
			held_until: new Date(2026, 9, 13, 12, 35, 0).getTime()
		});
		const view = render(NotificationState, { notification: held, now: NOW });
		await expect.element(view.container).toHaveTextContent('held until 12:35');
		await expect.element(page.getByText('rate limit: 5 per 300 s already sent')).toBeVisible();
	});
});

describe('NotificationActions', () => {
	it('offers what the state allows', async () => {
		render(NotificationActions, { notification: notification('acknowledged'), onchange: vi.fn() });
		await expect.element(page.getByRole('button', { name: /^Resolve/ })).toBeVisible();
		await expect.element(page.getByRole('button', { name: /^Silence events like/ })).toBeVisible();
		await expect.element(page.getByRole('button', { name: /^Acknowledge/ })).not.toBeInTheDocument();
	});

	it('acknowledges a notification and reports the change', async () => {
		apiMock.mockResolvedValue(undefined);
		const onchange = vi.fn();
		render(NotificationActions, { notification: notification('sent'), onchange });
		await page.getByRole('button', { name: /^Acknowledge/ }).click();
		expect(apiMock).toHaveBeenCalledWith('POST', '/api/notifications/n1/acknowledge');
		await vi.waitFor(() => expect(onchange).toHaveBeenCalledOnce());
	});

	it('reports the change even when the server refuses, so the row is read again', async () => {
		apiMock.mockRejectedValue(new Error('the notification is already acknowledged'));
		const onchange = vi.fn();
		render(NotificationActions, { notification: notification('sent'), onchange });
		await page.getByRole('button', { name: /^Resolve/ }).click();
		await vi.waitFor(() => expect(onchange).toHaveBeenCalledOnce());
	});

	it('opens the silence form filled in from the first event', async () => {
		apiMock.mockResolvedValue({ events: [{ topic: 'orders.failed', labels: { env: 'prod' } }] });
		render(NotificationActions, { notification: notification('sent'), onchange: vi.fn() });
		await page.getByRole('button', { name: /^Silence events like/ }).click();
		expect(apiMock).toHaveBeenCalledWith('GET', '/api/notifications/n1');
		await vi.waitFor(() => expect(gotoMock).toHaveBeenCalledWith('/silences?topic=orders.failed&label=env%3Dprod'));
	});
});
