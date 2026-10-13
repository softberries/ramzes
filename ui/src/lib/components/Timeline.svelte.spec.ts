import { describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Attempt, DeliveryState, Journey, JourneyDelivery } from '#lib/api/types.ts';
import Timeline from './Timeline.svelte';

const NOW = 1_791_884_500_000;

function attempt(id: string, outcome: string, status: number | null, detail: string | null): Attempt {
	return { id, delivery_id: 'd1', started_at: NOW - 30_000, duration_ms: 212, outcome, status, detail };
}

function delivery(state: DeliveryState, attempts: Attempt[], extra: Partial<JourneyDelivery> = {}): JourneyDelivery {
	return {
		id: 'd1',
		workspace_id: 'w',
		notification_id: 'n1',
		connection_id: 'c1',
		destination: null,
		state,
		attempts,
		next_attempt_at: NOW,
		last_error: null,
		external_ref: null,
		created_at: NOW - 30_000,
		updated_at: NOW - 30_000,
		...extra
	} as JourneyDelivery;
}

function journey(deliveries: JourneyDelivery[] | null): Journey {
	return {
		event: {
			id: 'e1',
			workspace_id: 'w',
			source_id: 's1',
			topic: 'orders.failed',
			severity: 'error',
			status: 'firing',
			title: 'Charge failed',
			body: null,
			labels: {},
			payload: null,
			dedup_key: null,
			occurred_at: NOW - 31_000,
			received_at: NOW - 31_000
		},
		routed: true,
		route_count: deliveries ? 1 : 0,
		notifications: deliveries
			? [
					{
						id: 'n1',
						route_id: 'r1',
						route_name: 'Payment failures',
						connection_id: 'c1',
						destination: null,
						state: 'sent',
						reason: null,
						replay: false,
						role: 'first',
						event_count: 1,
						created_at: NOW - 30_500,
						deliveries
					}
				]
			: []
	};
}

const names = { connections: { c1: 'ops channel' }, sources: { s1: 'orders-api' } };

function show(j: Journey, onreplay = vi.fn()) {
	render(Timeline, { journey: j, ...names, now: NOW, onreplay });
	return onreplay;
}

describe('Timeline', () => {
	it('shows a delivered event from receipt to the service answer', async () => {
		show(journey([delivery('delivered', [attempt('a1', 'delivered', 200, null)])]));
		await expect.element(page.getByText('Received from orders-api')).toBeInTheDocument();
		await expect.element(page.getByText('Matched route Payment failures')).toBeInTheDocument();
		await expect.element(page.getByText('ops channel')).toBeInTheDocument();
		await expect.element(page.getByText('delivered', { exact: true }).first()).toBeInTheDocument();
		await expect.element(page.getByText('HTTP 200')).toBeInTheDocument();
		await expect.element(page.getByText('212 ms')).toBeInTheDocument();
	});

	it('shows a retry with its error and when the next attempt is due', async () => {
		const retrying = delivery('pending', [attempt('a1', 'retry', 503, 'HTTP 503: upstream down')], {
			next_attempt_at: NOW + 5 * 60_000,
			last_error: 'HTTP 503: upstream down'
		});
		show(journey([retrying]));
		await expect.element(page.getByText('HTTP 503: upstream down')).toBeInTheDocument();
		await expect.element(page.getByText('Next attempt in 5 min')).toBeInTheDocument();
	});

	it('shows a failure and lets it be sent again', async () => {
		const failed = delivery('failed', [attempt('a1', 'failed', 401, 'HTTP 401: invalid_token')], {
			last_error: 'HTTP 401: invalid_token'
		});
		const onreplay = show(journey([failed]));
		await expect.element(page.getByText('failed', { exact: true }).first()).toBeInTheDocument();
		await expect.element(page.getByText('HTTP 401: invalid_token')).toBeInTheDocument();
		await page.getByRole('button', { name: 'Send again' }).click();
		expect(onreplay).toHaveBeenCalledWith('d1');
	});

	it('explains an event that matched no route', async () => {
		show(journey(null));
		await expect.element(page.getByText('No route matched')).toBeInTheDocument();
		await expect.element(page.getByRole('link', { name: 'Routes' })).toBeInTheDocument();
	});

	it('says so while the event has not been routed yet', async () => {
		show({ ...journey(null), routed: false });
		await expect.element(page.getByText('Waiting to be routed')).toBeInTheDocument();
	});

	it('shows the message each target received, with its request folded away', async () => {
		const j = journey([delivery('delivered', [attempt('a1', 'delivered', 200, null)])]);
		const previews = {
			n1: {
				notification_id: 'n1',
				connection_id: 'c1',
				title: '[prod] Charge failed',
				body: '14 charges failed',
				warning: 'the template failed, so the default wording was sent',
				request: {
					method: 'POST',
					url: 'https://hooks.example.com/T000',
					headers: [['content-type', 'application/json']] as [string, string][],
					body: '{"text":"[prod] Charge failed"}'
				}
			}
		};
		render(Timeline, { journey: j, ...names, previews, now: NOW, onreplay: vi.fn() });
		await expect.element(page.getByText('[prod] Charge failed', { exact: true })).toBeVisible();
		await expect.element(page.getByText('14 charges failed')).toBeVisible();
		await expect.element(page.getByText('the template failed, so the default wording was sent')).toBeVisible();
		await expect.element(page.getByText('https://hooks.example.com/T000')).not.toBeVisible();
		await page.getByText('Request', { exact: true }).click();
		await expect.element(page.getByText('https://hooks.example.com/T000')).toBeVisible();
		await expect.element(page.getByText('content-type:')).toBeVisible();
	});

	it('says why the request cannot be shown and shows a delivery warning', async () => {
		const j = journey([delivery('delivered', [], { warning: 'the message was shortened to 1000 characters' })]);
		const previews = {
			n1: { notification_id: 'n1', connection_id: 'c1', title: 'Charge failed', request_error: 'the connection was deleted' }
		};
		render(Timeline, { journey: j, ...names, previews, now: NOW, onreplay: vi.fn() });
		await expect.element(page.getByText('The request cannot be shown: the connection was deleted')).toBeVisible();
		await expect.element(page.getByText('the message was shortened to 1000 characters')).toBeVisible();
	});

	it('says how the event relates to a notification it did not open, and links to it', async () => {
		const j = journey([]);
		j.notifications[0] = { ...j.notifications[0], role: 'grouped', event_count: 4 };
		show(j);
		await expect.element(page.getByTestId('role')).toHaveTextContent('Joined a group of 4 events.');
		await expect
			.element(page.getByRole('link', { name: 'Open the notification' }))
			.toHaveAttribute('href', '/notifications/n1');
	});

	it('says nothing more about the event that opened a notification', async () => {
		show(journey([delivery('delivered', [])]));
		await expect.element(page.getByRole('link', { name: 'Open the notification' })).toBeVisible();
		await expect.element(page.getByTestId('role')).not.toBeInTheDocument();
	});

	it('says why a suppressed notification was not sent', async () => {
		const j = journey([]);
		j.notifications[0] = { ...j.notifications[0], state: 'suppressed', reason: 'silenced: deploy' };
		show(j);
		await expect.element(page.getByText('Not sent')).toBeVisible();
		await expect.element(page.getByText('silenced: deploy')).toBeVisible();
	});

	it('says until when a held notification waits', async () => {
		const j = journey([]);
		j.notifications[0] = {
			...j.notifications[0],
			state: 'held',
			reason: 'rate limit: 5 per 300 s already sent',
			held_until: NOW + 60_000
		};
		show(j);
		await expect.element(page.getByText(/^Held until \d\d:\d\d$/)).toBeVisible();
		await expect.element(page.getByText('rate limit: 5 per 300 s already sent')).toBeVisible();
	});

	it('names a connection that no longer exists', async () => {
		const j = journey([delivery('failed', [], { last_error: 'the connection was deleted' })]);
		render(Timeline, { journey: j, connections: {}, sources: {}, now: NOW, onreplay: vi.fn() });
		await expect.element(page.getByText('a deleted connection')).toBeInTheDocument();
		await expect.element(page.getByText('Received from a deleted source')).toBeInTheDocument();
	});
});
