import { describe, expect, it } from 'vitest';
import type { EscalationStep, NotificationRow, StepProgress } from './api/types.ts';
import {
	chainToForm,
	cumulativeSeconds,
	delaysOf,
	emptyStep,
	firstSentAt,
	formToChain,
	sinceFirstWords,
	stepLabel,
	stepState,
	stepWords,
	stoppedWords,
	summarizeChain,
	targetWords
} from './escalation.ts';

const NOW = new Date(2026, 9, 13, 14, 36, 0).getTime();
const at = (hours: number, minutes: number, seconds = 0) => new Date(2026, 9, 13, hours, minutes, seconds).getTime();

const takesDestination = (id: string) => id !== 'email';

describe('the chain and its form', () => {
	const chain: EscalationStep[] = [
		{ after_s: 900, targets: [{ connection_id: 'lead', destination: 'oncall' }] },
		{ after_s: 7200, targets: [{ connection_id: 'boss', template_id: 't1' }, { connection_id: 'email' }] }
	];

	it('is empty for an untouched form', () => {
		expect(chainToForm(undefined)).toEqual([]);
		expect(formToChain(chainToForm(undefined), takesDestination)).toEqual([]);
		expect(formToChain(chainToForm([]), takesDestination)).toEqual([]);
	});

	it('shows a stored chain in the largest unit that fits and gives it back unchanged', () => {
		const form = chainToForm(chain);
		expect(form[0].after).toEqual({ amount: 15, unit: 'min' });
		expect(form[1].after).toEqual({ amount: 2, unit: 'h' });
		expect(form[0].targets).toEqual([{ connection_id: 'lead', destination: 'oncall', template_id: '' }]);
		expect(formToChain(form, takesDestination)).toEqual(chain);
	});

	it('starts a new step at 15 minutes with one target to choose', () => {
		expect(emptyStep()).toEqual({
			after: { amount: 15, unit: 'min' },
			targets: [{ connection_id: '', destination: '', template_id: '' }]
		});
	});

	it('converts the delay to seconds and keeps the order of the steps', () => {
		const steps = [emptyStep(), emptyStep(), emptyStep()];
		steps[0].after = { amount: 30, unit: 's' };
		steps[0].targets[0].connection_id = 'a';
		steps[1].after = { amount: 5, unit: 'min' };
		steps[1].targets[0].connection_id = 'b';
		steps[2].after = { amount: 1, unit: 'h' };
		steps[2].targets[0].connection_id = 'c';
		expect(formToChain(steps, takesDestination)).toEqual([
			{ after_s: 30, targets: [{ connection_id: 'a' }] },
			{ after_s: 300, targets: [{ connection_id: 'b' }] },
			{ after_s: 3600, targets: [{ connection_id: 'c' }] }
		]);
	});

	it('cleans targets as a route does: no unchosen rows, no destination the connector does not take', () => {
		const step = emptyStep();
		step.targets = [
			{ connection_id: 'email', destination: 'ignored', template_id: '' },
			{ connection_id: '', destination: 'x', template_id: '' },
			{ connection_id: 'lead', destination: ' oncall ', template_id: 't1' }
		];
		expect(formToChain([step], takesDestination)[0].targets).toEqual([
			{ connection_id: 'email' },
			{ connection_id: 'lead', destination: 'oncall', template_id: 't1' }
		]);
	});

	it('keeps a step that is not filled in, so the server can say what is missing', () => {
		const step = emptyStep();
		step.after.amount = null;
		expect(formToChain([step], takesDestination)).toEqual([{ after_s: 0, targets: [] }]);
	});
});

describe('delays', () => {
	it('reads the delay of each step of a form in seconds', () => {
		const steps = [emptyStep(), emptyStep()];
		steps[1].after = { amount: null, unit: 'h' };
		expect(delaysOf(steps)).toEqual([900, 0]);
	});
});

describe('cumulativeSeconds', () => {
	it('adds each delay to the ones before it', () => {
		expect(cumulativeSeconds([900, 1800, 0])).toEqual([900, 2700, 2700]);
		expect(cumulativeSeconds([])).toEqual([]);
	});
});

describe('sinceFirstWords', () => {
	it('says how long after the first message a step goes out', () => {
		expect(sinceFirstWords(2700)).toBe('45 min after the first message');
		expect(sinceFirstWords(7200)).toBe('2 h after the first message');
		// Delays that do not add up to a round number are not shown as a heap of seconds.
		expect(sinceFirstWords(90)).toBe('1 min 30 s after the first message');
		expect(sinceFirstWords(2703)).toBe('45 min 3 s after the first message');
		expect(sinceFirstWords(5400)).toBe('1 h 30 min after the first message');
	});
});

describe('summarizeChain', () => {
	const names = { lead: 'ops-lead', boss: 'head-of-eng' };

	it('is empty without a chain', () => {
		expect(summarizeChain(undefined, names)).toBe('');
		expect(summarizeChain([], names)).toBe('');
	});

	it('names who is told and how long after the first message', () => {
		const chain: EscalationStep[] = [
			{ after_s: 900, targets: [{ connection_id: 'lead' }] },
			{ after_s: 1800, targets: [{ connection_id: 'boss' }] }
		];
		expect(summarizeChain(chain, names)).toBe('then ops-lead after 15 min, then head-of-eng after 45 min');
	});

	it('adds up delays in different units', () => {
		const chain: EscalationStep[] = [
			{ after_s: 3, targets: [{ connection_id: 'lead' }] },
			{ after_s: 2700, targets: [{ connection_id: 'boss' }] }
		];
		expect(summarizeChain(chain, names)).toBe('then ops-lead after 3 s, then head-of-eng after 45 min 3 s');
	});

	it('lists several targets of a step with their destinations and admits a deleted connection', () => {
		const chain: EscalationStep[] = [
			{ after_s: 60, targets: [{ connection_id: 'lead', destination: 'oncall' }, { connection_id: 'gone' }] }
		];
		expect(summarizeChain(chain, names)).toBe('then ops-lead (oncall) and a deleted connection after 1 min');
	});
});

describe('progress of a step', () => {
	const step = (extra: Partial<StepProgress> = {}): StepProgress => ({
		after_s: 300,
		targets: [{ connection_id: 'lead', connection_name: 'lead', destination: 'oncall' }],
		...extra
	});

	it('is sent, due or not reached', () => {
		expect(stepState(step({ sent_at: at(14, 32) }))).toBe('sent');
		expect(stepState(step({ due_at: at(14, 40) }))).toBe('due');
		expect(stepState(step({ sent_at: null, due_at: null }))).toBe('not_reached');
		// A step that went out is sent even while a timer for it lingers.
		expect(stepState(step({ sent_at: at(14, 32), due_at: at(14, 32) }))).toBe('sent');
	});

	it('says when it was sent', () => {
		expect(stepWords(step({ sent_at: at(14, 32, 20) }), NOW)).toBe('sent at 14:32');
	});

	it('counts down to when it is due', () => {
		expect(stepWords(step({ due_at: at(14, 40) }), NOW)).toBe('due in 4 min');
		expect(stepWords(step({ due_at: at(14, 40, 12) }), NOW)).toBe('due in 4 min 12 s');
		expect(stepWords(step({ due_at: at(14, 36, 9) }), NOW)).toBe('due in 9 s');
		expect(stepWords(step({ due_at: at(14, 35) }), NOW)).toBe('due now');
	});

	it('says when it was not reached', () => {
		expect(stepWords(step(), NOW)).toBe('not reached');
	});

	it('names the targets of a step', () => {
		expect(targetWords(step().targets)).toBe('lead (oncall)');
		expect(
			targetWords([
				{ connection_id: 'a', connection_name: 'lead' },
				{ connection_id: 'b', connection_name: null },
				{ connection_id: 'c', connection_name: 'boss' }
			])
		).toBe('lead, a deleted connection and boss');
	});
});

describe('stoppedWords', () => {
	const row = (extra: Partial<NotificationRow>): NotificationRow => ({
		id: 'n1',
		route_id: 'r1',
		route_name: 'Paged',
		connection_id: 'c1',
		state: 'sent',
		title: 'Database down',
		severity: 'critical',
		event_count: 1,
		sent_count: 1,
		created_at: at(14, 30),
		updated_at: at(14, 30),
		...extra
	});
	const open: StepProgress[] = [
		{ after_s: 300, targets: [], sent_at: at(14, 32) },
		{ after_s: 600, targets: [] }
	];

	it('says nothing while the chain is still running', () => {
		expect(stoppedWords(row({}), open, NOW)).toBe('');
		expect(stoppedWords(row({ state: 'grouping' }), open, NOW)).toBe('');
	});

	it('says who acknowledged it and when', () => {
		const acked = row({ state: 'acknowledged', acked_by: 'kris@example.com', acked_at: at(14, 35) });
		expect(stoppedWords(acked, open, NOW)).toBe('stopped: acknowledged by kris@… at 14:35');
	});

	it('says when it was resolved', () => {
		expect(stoppedWords(row({ state: 'resolved', resolved_at: at(14, 34) }), open, NOW)).toBe('stopped: resolved at 14:34');
	});

	it('names the acknowledgement when a notification was acknowledged and resolved later', () => {
		const both = row({ state: 'resolved', acked_by: 'link', acked_at: at(14, 33), resolved_at: at(14, 35) });
		expect(stoppedWords(both, open, NOW)).toBe('stopped: acknowledged by link at 14:33');
	});

	it('does not call a chain stopped when every step went out', () => {
		const done: StepProgress[] = [{ after_s: 300, targets: [], sent_at: at(14, 32) }];
		expect(stoppedWords(row({ state: 'acknowledged', acked_by: 'x', acked_at: at(14, 35) }), done, NOW)).toBe('');
	});
});

describe('deliveries of a step', () => {
	const names = { lead: 'ops-lead' };

	it('labels a delivery with its step, counted from one, and where it went', () => {
		expect(stepLabel({ step: 0, connection_id: 'lead', destination: 'oncall' }, names)).toBe('Step 1 to ops-lead (oncall)');
		expect(stepLabel({ step: 1, connection_id: 'gone' }, names)).toBe('Step 2 to a deleted connection');
	});

	it("does not label a delivery to the route's own targets", () => {
		expect(stepLabel({ connection_id: 'lead' }, names)).toBe('');
		expect(stepLabel({ step: null, connection_id: 'lead' }, names)).toBe('');
	});

	it('finds when the first message went out', () => {
		expect(
			firstSentAt([
				{ step: 0, created_at: 50 },
				{ step: null, created_at: 30 },
				{ created_at: 20 }
			])
		).toBe(20);
		expect(firstSentAt([{ step: 0, created_at: 50 }])).toBeNull();
		expect(firstSentAt([])).toBeNull();
	});
});
