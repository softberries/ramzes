import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Attempt, ConnectionView, EscalationStep, JourneyDelivery, NotificationView, StepProgress } from '#lib/api/types.ts';
import EscalationEditorHarness from './EscalationEditorHarness.svelte';
import EscalationProgress from './EscalationProgress.svelte';

const at = (hours: number, minutes: number, seconds = 0) => new Date(2026, 9, 13, hours, minutes, seconds).getTime();
const NOW = at(14, 36);

function connection(id: string, name: string): ConnectionView {
	return { id, kind: 'webhook', name, settings: {}, has_credentials: false, failure_streak: 0, created_at: 1, updated_at: 1 };
}

const connections = [connection('lead', 'ops-lead'), connection('boss', 'head-of-eng'), connection('chat', 'chat')];

describe('EscalationEditor', () => {
	/** Renders the editor and returns a reader for the chain it currently holds. */
	function editing(initial?: EscalationStep[]) {
		render(EscalationEditorHarness, { initial, connections, destinations: { chat: 'Channel' } });
		return async () => JSON.parse((await page.getByTestId('chain').element()).textContent ?? 'null') as EscalationStep[];
	}

	it('starts without steps and produces an empty chain', async () => {
		const chain = editing();
		expect(await chain()).toEqual([]);
		await expect.element(page.getByRole('button', { name: 'Add a step' })).toBeVisible();
		await expect.element(page.getByText('counts from the step before it')).toBeVisible();
		await expect.element(page.getByRole('list', { name: 'Escalation steps' })).not.toBeInTheDocument();
	});

	it('adds a step in minutes and builds the chain from what is chosen', async () => {
		const chain = editing();
		await page.getByRole('button', { name: 'Add a step' }).click();
		await expect.element(page.getByLabelText('Step 1: delay')).toHaveValue(15);
		await expect.element(page.getByLabelText('Step 1: unit')).toHaveValue('min');
		await page.getByLabelText('Step 1: delay').fill('5');
		await page.getByLabelText('Step 1: Connection').selectOptions('lead');
		expect(await chain()).toEqual([{ after_s: 300, targets: [{ connection_id: 'lead' }] }]);
		await page.getByLabelText('Step 1: unit').selectOptions('h');
		expect((await chain())[0].after_s).toBe(18_000);
	});

	it('asks for a destination only when the connection takes one', async () => {
		const chain = editing();
		await page.getByRole('button', { name: 'Add a step' }).click();
		await page.getByLabelText('Step 1: Connection').selectOptions('lead');
		await expect.element(page.getByLabelText('Step 1: Channel')).not.toBeInTheDocument();
		await page.getByLabelText('Step 1: Connection').selectOptions('chat');
		await page.getByLabelText('Step 1: Channel').fill('#oncall');
		expect((await chain())[0].targets).toEqual([{ connection_id: 'chat', destination: '#oncall' }]);
	});

	it('tells several targets in one step', async () => {
		const chain = editing();
		await page.getByRole('button', { name: 'Add a step' }).click();
		await page.getByLabelText('Step 1: Connection').selectOptions('lead');
		await page.getByRole('button', { name: 'Add another connection' }).click();
		await page.getByLabelText('Step 1: Connection').nth(1).selectOptions('boss');
		expect((await chain())[0].targets).toEqual([{ connection_id: 'lead' }, { connection_id: 'boss' }]);
	});

	it('shows how long after the first message each step goes out', async () => {
		editing([
			{ after_s: 900, targets: [{ connection_id: 'lead' }] },
			{ after_s: 1800, targets: [{ connection_id: 'boss' }] }
		]);
		const since = page.getByTestId('since-first');
		await expect.element(since.nth(0)).toHaveTextContent('15 min after the first message');
		await expect.element(since.nth(1)).toHaveTextContent('45 min after the first message');
		await expect
			.element(page.getByTestId('summary'))
			.toHaveTextContent('then ops-lead after 15 min, then head-of-eng after 45 min');
		// Making the first step later moves the second with it.
		await page.getByLabelText('Step 1: delay').fill('20');
		await expect.element(since.nth(1)).toHaveTextContent('50 min after the first message');
	});

	it('removes a step and keeps the others in order', async () => {
		const chain = editing([
			{ after_s: 60, targets: [{ connection_id: 'lead' }] },
			{ after_s: 120, targets: [{ connection_id: 'boss', destination: 'x' }] },
			{ after_s: 7200, targets: [{ connection_id: 'chat', destination: '#all' }] }
		]);
		await expect.element(page.getByLabelText('Step 3: delay')).toHaveValue(2);
		await expect.element(page.getByLabelText('Step 3: unit')).toHaveValue('h');
		await page.getByRole('button', { name: 'Remove step 2' }).click();
		expect(await chain()).toEqual([
			{ after_s: 60, targets: [{ connection_id: 'lead' }] },
			{ after_s: 7200, targets: [{ connection_id: 'chat', destination: '#all' }] }
		]);
		await page.getByRole('button', { name: 'Add another step' }).click();
		expect((await chain()).map((step) => step.after_s)).toEqual([60, 7200, 900]);
	});
});

describe('EscalationProgress', () => {
	const steps = (first: Partial<StepProgress>, second: Partial<StepProgress>): StepProgress[] => [
		{ after_s: 300, targets: [{ connection_id: 'lead', connection_name: 'ops-lead', destination: 'oncall' }], ...first },
		{ after_s: 600, targets: [{ connection_id: 'boss', connection_name: 'head-of-eng' }], ...second }
	];

	function delivery(id: string, step: number | null, created_at: number): JourneyDelivery {
		// The generated type takes `attempts` for a count and a list at once; the API sends the list.
		return {
			id,
			workspace_id: 'w',
			notification_id: 'n1',
			connection_id: 'c1',
			state: 'delivered',
			attempts: [] as Attempt[],
			next_attempt_at: created_at,
			created_at,
			updated_at: created_at,
			step
		} as JourneyDelivery;
	}

	function notification(extra: Partial<NotificationView>): NotificationView {
		return {
			id: 'n1',
			route_id: 'r1',
			route_name: 'Paged',
			connection_id: 'c1',
			connection_name: 'ops',
			state: 'sent',
			title: 'Database down',
			severity: 'critical',
			event_count: 1,
			sent_count: 1,
			created_at: at(14, 30),
			updated_at: at(14, 30),
			events: [],
			deliveries: [delivery('d1', null, at(14, 30))],
			escalation: steps({}, {}),
			...extra
		};
	}

	const states = () => page.getByTestId('step-state');

	it('shows the first message and counts down to the step that is due', async () => {
		const view = render(EscalationProgress, {
			notification: notification({ escalation: steps({ due_at: at(14, 40) }, {}) }),
			now: NOW
		});
		const items = page.getByRole('list', { name: 'Escalation chain' }).getByRole('listitem');
		await expect.element(items.nth(0)).toHaveTextContent(/First message\s+to ops\s+sent at 14:30/);
		await expect.element(items.nth(1)).toHaveTextContent('Step 1');
		await expect.element(items.nth(1)).toHaveTextContent('to ops-lead (oncall), 5 min after the first message');
		await expect.element(states().nth(0)).toHaveTextContent('due in 4 min');
		await expect.element(items.nth(2)).toHaveTextContent('to head-of-eng, 15 min after the first message');
		await expect.element(states().nth(1)).toHaveTextContent('not reached');
		await expect.element(page.getByTestId('chain-stopped')).not.toBeInTheDocument();

		// The countdown follows the clock it is given; nothing is fetched.
		await view.rerender({ now: NOW + 1000 });
		await expect.element(states().nth(0)).toHaveTextContent('due in 3 min 59 s');
		await view.rerender({ now: at(14, 40, 5) });
		await expect.element(states().nth(0)).toHaveTextContent('due now');
	});

	it('shows a step that went out and the next one due', async () => {
		render(EscalationProgress, {
			notification: notification({ escalation: steps({ sent_at: at(14, 35) }, { due_at: at(14, 45) }) }),
			now: NOW
		});
		await expect.element(states().nth(0)).toHaveTextContent('sent at 14:35');
		await expect.element(states().nth(1)).toHaveTextContent('due in 9 min');
	});

	it('says that the chain stopped, where it stopped and why', async () => {
		render(EscalationProgress, {
			notification: notification({
				state: 'acknowledged',
				acked_by: 'kris@example.com',
				acked_at: at(14, 35, 30),
				escalation: steps({ sent_at: at(14, 35) }, {})
			}),
			now: NOW
		});
		const items = page.getByRole('list', { name: 'Escalation chain' }).getByRole('listitem');
		await expect.element(items.nth(1)).toHaveTextContent('sent at 14:35');
		await expect.element(items.nth(2)).toHaveTextContent('stopped: acknowledged by kris@… at 14:35');
		await expect.element(items.nth(3)).toHaveTextContent('not reached');
		expect(items.elements().length).toBe(4);
	});

	it('says that a resolved notification stopped the chain before its first step', async () => {
		render(EscalationProgress, {
			notification: notification({ state: 'resolved', resolved_at: at(14, 33) }),
			now: NOW
		});
		const items = page.getByRole('list', { name: 'Escalation chain' }).getByRole('listitem');
		await expect.element(items.nth(1)).toHaveTextContent('stopped: resolved at 14:33');
		await expect.element(states().nth(0)).toHaveTextContent('not reached');
	});

	it('says that nothing was sent yet while the notification waits', async () => {
		render(EscalationProgress, { notification: notification({ state: 'held', deliveries: [] }), now: NOW });
		const items = page.getByRole('list', { name: 'Escalation chain' }).getByRole('listitem');
		await expect.element(items.nth(0)).toHaveTextContent('not sent yet');
		await expect.element(states().nth(0)).toHaveTextContent('not reached');
	});
});
