import { describe, expect, it } from 'vitest';
import { ago, clock, duration, outcomeOf, timeOfDay, until } from './format';

describe('clock', () => {
	it('shows local time with seconds', () => {
		const ms = new Date(2026, 9, 13, 9, 41, 7).getTime();
		expect(clock(ms)).toBe('09:41:07');
	});

	it('adds the date when the time is not today', () => {
		const now = new Date(2026, 9, 13, 12, 0, 0).getTime();
		const yesterday = new Date(2026, 9, 12, 23, 5, 9).getTime();
		expect(clock(yesterday, now)).toBe('Oct 12 23:05:09');
		expect(clock(new Date(2026, 9, 13, 0, 0, 1).getTime(), now)).toBe('00:00:01');
	});
});

describe('timeOfDay', () => {
	it('shows hours and minutes, with the date when it is not today', () => {
		const now = new Date(2026, 9, 13, 12, 0, 0).getTime();
		expect(timeOfDay(new Date(2026, 9, 13, 7, 5, 59).getTime(), now)).toBe('07:05');
		expect(timeOfDay(new Date(2026, 9, 14, 7, 0, 0).getTime(), now)).toBe('Oct 14 07:00');
	});
});

describe('ago', () => {
	it('picks the largest fitting unit', () => {
		const now = 10_000_000_000;
		expect(ago(now - 400, now)).toBe('just now');
		expect(ago(now - 42_000, now)).toBe('42 s ago');
		expect(ago(now - 5 * 60_000, now)).toBe('5 min ago');
		expect(ago(now - 3 * 3_600_000, now)).toBe('3 h ago');
		expect(ago(now - 4 * 86_400_000, now)).toBe('4 d ago');
	});
});

describe('until', () => {
	it('describes a future time', () => {
		const now = 10_000_000_000;
		expect(until(now + 5_000, now)).toBe('in 5 s');
		expect(until(now + 8 * 60_000, now)).toBe('in 8 min');
		expect(until(now - 1, now)).toBe('now');
	});
});

describe('duration', () => {
	it('uses milliseconds below a second and seconds above', () => {
		expect(duration(212)).toBe('212 ms');
		expect(duration(1500)).toBe('1.5 s');
		expect(duration(0)).toBe('0 ms');
	});
});

describe('outcomeOf', () => {
	const row = {
		routed: true,
		route_count: 1,
		deliveries_total: 2,
		deliveries_delivered: 2,
		deliveries_failed: 0,
		notifications_waiting: 0,
		notifications_suppressed: 0
	};
	const unsent = { ...row, deliveries_total: 0, deliveries_delivered: 0 };

	it('is pending until the event is routed', () => {
		expect(outcomeOf({ ...row, routed: false })).toBe('pending');
	});

	it('is unrouted when no route matched', () => {
		expect(outcomeOf({ ...unsent, route_count: 0 })).toBe('unrouted');
	});

	it('is failed as soon as one delivery failed for good', () => {
		expect(outcomeOf({ ...row, deliveries_delivered: 1, deliveries_failed: 1 })).toBe('failed');
	});

	it('is delivered only when every delivery arrived', () => {
		expect(outcomeOf(row)).toBe('delivered');
		expect(outcomeOf({ ...row, deliveries_delivered: 1 })).toBe('pending');
	});

	it('is not sent when every notification of the event was suppressed', () => {
		expect(outcomeOf({ ...unsent, notifications_suppressed: 1 })).toBe('not_sent');
		expect(outcomeOf({ ...unsent, route_count: 2, notifications_suppressed: 2 })).toBe('not_sent');
	});

	it('is not "not sent" when another route did send it', () => {
		expect(outcomeOf({ ...row, deliveries_total: 1, deliveries_delivered: 1, notifications_suppressed: 1 })).toBe('delivered');
	});

	it('is waiting while a notification is collecting or held', () => {
		expect(outcomeOf({ ...unsent, notifications_waiting: 1 })).toBe('waiting');
		expect(outcomeOf({ ...unsent, notifications_waiting: 1, notifications_suppressed: 1 })).toBe('waiting');
		// One route delivered already, another still holds its message.
		expect(outcomeOf({ ...row, notifications_waiting: 1 })).toBe('waiting');
	});

	it('is failed rather than waiting when a delivery failed', () => {
		expect(outcomeOf({ ...row, deliveries_delivered: 1, deliveries_failed: 1, notifications_waiting: 1 })).toBe('failed');
	});

	it('is pending when a matched event has nothing to show yet', () => {
		expect(outcomeOf(unsent)).toBe('pending');
	});
});
