import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { RouteInput, RouteResult } from '#lib/api/types.ts';

const { apiMock } = vi.hoisted(() => ({ apiMock: vi.fn() }));
vi.mock('#lib/api/client.ts', async (original) => ({
	...(await original<typeof import('#lib/api/client.ts')>()),
	api: apiMock
}));

import { ApiError } from '#lib/api/client.ts';
import RouteTester from './RouteTester.svelte';

const ROUTES: RouteInput[] = [
	{ name: 'Big orders', enabled: true, stop: true, match: { expr: 'payload.amount > 100' }, targets: [] },
	{ name: 'Rest', enabled: true, stop: false, match: {}, targets: [] }
];

const RESULTS: RouteResult[] = [
	{
		name: 'Big orders',
		outcome: 'matched',
		targets: [
			{
				connection_id: 'c1',
				connection_name: 'ops',
				destination: '#alerts',
				title: 'Order 17 failed',
				warning: null,
				outcome: 'sent',
				steps: []
			},
			{
				connection_id: 'c2',
				connection_name: null,
				title: 'Order 17 failed',
				warning: 'the template failed',
				outcome: 'sent',
				steps: []
			}
		]
	},
	{ name: 'Rest', outcome: 'skipped', targets: [] }
];

/** Answers the list of recent events and the test request. */
function answering(events: { id: string; title: string; received_at: number }[], test: () => unknown) {
	apiMock.mockImplementation(async (_method: string, path: string) =>
		path.startsWith('/api/events') ? { events } : test()
	);
}

const testCalls = () => apiMock.mock.calls.filter((call) => call[1] === '/api/routes/test');

beforeEach(() => {
	apiMock.mockReset();
});

describe('RouteTester', () => {
	it('sends the written event with every route and shows what each would do', async () => {
		answering([], () => ({ results: RESULTS }));
		render(RouteTester, { routes: ROUTES, draftIndex: 0 });

		const rows = page.getByRole('list', { name: 'Result for each route' }).getByRole('listitem');
		await expect.element(rows.nth(0)).toHaveTextContent('Big orders');
		await expect.element(rows.nth(0)).toHaveTextContent('being edited');
		await expect.element(rows.nth(0)).toHaveTextContent('to ops (#alerts): Order 17 failed');
		await expect.element(rows.nth(0)).toHaveTextContent('to a deleted connection: Order 17 failed');
		await expect.element(rows.nth(0)).toHaveTextContent('the template failed');
		await expect.element(rows.nth(1)).toHaveTextContent('skipped, an earlier route stops here');

		const body = testCalls()[0][2];
		expect(body.routes).toEqual(ROUTES);
		expect(body.event).toMatchObject({ topic: 'orders.payment.failed' });
		expect(body.event_id).toBeUndefined();
	});

	it('starts from the newest stored event when there is one', async () => {
		answering([{ id: 'e9', title: 'Charge failed', received_at: 1 }], () => ({ results: RESULTS }));
		render(RouteTester, { routes: ROUTES });

		await expect.element(page.getByLabelText('Event', { exact: true })).toHaveValue('e9');
		await expect.element(page.getByText('skipped, an earlier route stops here')).toBeVisible();
		const body = testCalls().at(-1)![2];
		expect(body.event_id).toBe('e9');
		expect(body.event).toBeUndefined();
	});

	it('says so when the written event is not valid JSON, without asking the server', async () => {
		answering([], () => ({ results: RESULTS }));
		render(RouteTester, { routes: ROUTES });
		await expect.element(page.getByText('skipped, an earlier route stops here')).toBeVisible();
		const before = testCalls().length;

		await page.getByLabelText('Event as JSON').fill('{');
		await expect.element(page.getByRole('alert')).toHaveTextContent('The sample event is not valid JSON.');
		expect(testCalls().length).toBe(before);
	});

	it("shows the server's reason when a route cannot be tested", async () => {
		answering([], () => {
			throw new ApiError(422, 'invalid', 'route "Big orders": expression: unexpected end of input');
		});
		render(RouteTester, { routes: ROUTES });
		await expect
			.element(page.getByRole('alert'))
			.toHaveTextContent('route "Big orders": expression: unexpected end of input');
	});
});
