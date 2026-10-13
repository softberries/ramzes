import { describe, expect, it } from 'vitest';
import type { Shaping } from './api/types.ts';
import {
	emptyShapingForm,
	formToShaping,
	outcomeTone,
	shapingToForm,
	splitSeconds,
	spanWords,
	stepWords,
	summarizeShaping
} from './shaping.ts';

const TZ = 'Europe/Warsaw';

const FULL: Shaping = {
	dedup: { window_s: 600, key: 'labels.order' },
	group: { by: ['service', 'env'], wait_s: 30, repeat_s: 14_400 },
	quiet_hours: { tz: 'Europe/London', start: '22:00', end: '07:00', days: ['sat', 'sun'], bypass_severity: 'critical' },
	rate_limit: { max: 5, window_s: 300 },
	notify_resolved: false
};

describe('splitSeconds', () => {
	it('uses the largest unit that divides the seconds evenly', () => {
		expect(splitSeconds(30)).toEqual({ amount: 30, unit: 's' });
		expect(splitSeconds(60)).toEqual({ amount: 1, unit: 'min' });
		expect(splitSeconds(300)).toEqual({ amount: 5, unit: 'min' });
		expect(splitSeconds(90)).toEqual({ amount: 90, unit: 's' });
		expect(splitSeconds(3600)).toEqual({ amount: 1, unit: 'h' });
		expect(splitSeconds(5400)).toEqual({ amount: 90, unit: 'min' });
		expect(splitSeconds(86_400)).toEqual({ amount: 24, unit: 'h' });
	});
});

describe('spanWords', () => {
	it('writes a number of seconds in its largest even unit', () => {
		expect(spanWords(30)).toBe('30 s');
		expect(spanWords(300)).toBe('5 min');
		expect(spanWords(7200)).toBe('2 h');
		expect(spanWords(90)).toBe('90 s');
	});
});

describe('formToShaping', () => {
	it('gives an empty policy for an untouched form', () => {
		expect(formToShaping(emptyShapingForm(TZ))).toEqual({});
	});

	it('leaves out every part that is switched off, whatever its fields hold', () => {
		const form = shapingToForm(FULL, TZ);
		form.dedup.on = false;
		form.group.on = false;
		form.quiet.on = false;
		form.rate.on = false;
		form.notifyResolved = true;
		expect(formToShaping(form)).toEqual({});
	});

	it('converts amounts and units to seconds', () => {
		const form = emptyShapingForm(TZ);
		form.dedup.on = true;
		form.dedup.window = { amount: 2, unit: 'h' };
		form.rate.on = true;
		form.rate.max = 3;
		form.rate.window = { amount: 10, unit: 'min' };
		expect(formToShaping(form)).toEqual({ dedup: { window_s: 7200 }, rate_limit: { max: 3, window_s: 600 } });
	});

	it('splits the labels to group by on commas and drops empty ones', () => {
		const form = emptyShapingForm(TZ);
		form.group.on = true;
		form.group.by = ' service, env ,, ';
		form.group.wait = { amount: 45, unit: 's' };
		expect(formToShaping(form)).toEqual({ group: { by: ['service', 'env'], wait_s: 45 } });
	});

	it('sends the repeat interval only when one is given', () => {
		const form = emptyShapingForm(TZ);
		form.group.on = true;
		form.group.repeat = { amount: 4, unit: 'h' };
		expect(formToShaping(form).group?.repeat_s).toBe(14_400);
		form.group.repeat = { amount: null, unit: 'h' };
		expect(formToShaping(form).group).not.toHaveProperty('repeat_s');
	});

	it('starts quiet hours in the given time zone, every day, with no exception', () => {
		const form = emptyShapingForm(TZ);
		form.quiet.on = true;
		expect(formToShaping(form)).toEqual({ quiet_hours: { tz: TZ, start: '22:00', end: '07:00', days: [] } });
	});

	it('orders the chosen days from Monday and keeps the severity that is sent anyway', () => {
		const form = emptyShapingForm(TZ);
		form.quiet.on = true;
		form.quiet.days = ['sun', 'mon', 'sat'];
		form.quiet.bypass = 'error';
		expect(formToShaping(form).quiet_hours).toMatchObject({ days: ['mon', 'sat', 'sun'], bypass_severity: 'error' });
	});

	it('sends an amount left empty as zero, so the server says what is wrong', () => {
		const form = emptyShapingForm(TZ);
		form.dedup.on = true;
		form.dedup.window = { amount: null, unit: 'min' };
		expect(formToShaping(form)).toEqual({ dedup: { window_s: 0 } });
	});

	it('says so only when resolutions are not announced', () => {
		const form = emptyShapingForm(TZ);
		form.notifyResolved = false;
		expect(formToShaping(form)).toEqual({ notify_resolved: false });
	});
});

describe('shapingToForm', () => {
	it('shows the policy of a route without one as an untouched form', () => {
		expect(shapingToForm(undefined, TZ)).toEqual(emptyShapingForm(TZ));
		expect(shapingToForm({ notify_resolved: true }, TZ)).toEqual(emptyShapingForm(TZ));
	});

	it('switches on each part the policy has and fills its fields', () => {
		const form = shapingToForm(FULL, TZ);
		expect(form.dedup).toEqual({ on: true, window: { amount: 10, unit: 'min' }, key: 'labels.order' });
		expect(form.group).toEqual({
			on: true,
			by: 'service, env',
			wait: { amount: 30, unit: 's' },
			repeat: { amount: 4, unit: 'h' }
		});
		expect(form.quiet).toEqual({
			on: true,
			tz: 'Europe/London',
			start: '22:00',
			end: '07:00',
			days: ['sat', 'sun'],
			bypass: 'critical'
		});
		expect(form.rate).toEqual({ on: true, max: 5, window: { amount: 5, unit: 'min' } });
		expect(form.notifyResolved).toBe(false);
	});

	it('survives the round trip', () => {
		expect(formToShaping(shapingToForm(FULL, TZ))).toEqual(FULL);
	});
});

describe('summarizeShaping', () => {
	it('says nothing for an empty policy', () => {
		expect(summarizeShaping(undefined)).toBe('');
		expect(summarizeShaping({ notify_resolved: true })).toBe('');
	});

	it('describes grouping, quiet hours and the rate limit in one line', () => {
		expect(
			summarizeShaping({
				group: { by: ['service'], wait_s: 30 },
				quiet_hours: { tz: 'Europe/Warsaw', start: '22:00', end: '07:00' },
				rate_limit: { max: 5, window_s: 300 }
			})
		).toBe('groups by service for 30 s, quiet 22:00–07:00 Europe/Warsaw, at most 5 per 5 min');
	});

	it('describes every part of a full policy', () => {
		expect(summarizeShaping(FULL)).toBe(
			'counts duplicates for 10 min, groups by service and env for 30 s, sends again every 4 h, ' +
				'quiet 22:00–07:00 Europe/London on Sat and Sun except critical, at most 5 per 5 min, ' +
				'does not announce resolutions'
		);
	});

	it('describes a group without labels and a lower severity that is sent anyway', () => {
		expect(
			summarizeShaping({
				group: { by: [], wait_s: 120 },
				quiet_hours: { tz: 'UTC', start: '00:00', end: '06:00', days: [], bypass_severity: 'error' }
			})
		).toBe('groups everything for 2 min, quiet 00:00–06:00 UTC except error or higher');
	});
});

describe('the route tester', () => {
	it('names each shaping step in plain words', () => {
		expect(['silence', 'resolve', 'dedup', 'group', 'quiet_hours', 'rate_limit', 'send'].map(stepWords)).toEqual([
			'Silence',
			'Resolved event',
			'Duplicates',
			'Grouping',
			'Quiet hours',
			'Rate limit',
			'Send'
		]);
		expect(stepWords('something_new')).toBe('something new');
	});

	it('tells an outcome that sends from one that waits and one that sends nothing', () => {
		expect(outcomeTone('sent')).toBe('sends');
		expect(outcomeTone('resolves an open notification')).toBe('sends');
		expect(outcomeTone('opens a group')).toBe('waits');
		expect(outcomeTone('held')).toBe('waits');
		expect(outcomeTone('joins an open notification')).toBe('quiet');
		expect(outcomeTone('not sent')).toBe('quiet');
	});
});
