import { describe, expect, it } from 'vitest';
import type { NotificationRow, NotificationState } from './api/types.ts';
import {
	TABS,
	canAcknowledge,
	canResolve,
	journeyWords,
	roleWords,
	shortWho,
	stateQuery,
	stateWords,
	tabOf
} from './notifications.ts';

const NOW = new Date(2026, 9, 13, 12, 0, 0).getTime();

function row(state: NotificationState, extra: Partial<NotificationRow> = {}): NotificationRow {
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
		sent_count: 0,
		created_at: NOW - 600_000,
		updated_at: NOW - 180_000,
		...extra
	};
}

describe('tabs', () => {
	it('offers the six views in order', () => {
		expect(TABS.map((tab) => tab.label)).toEqual(['Waiting', 'Sent', 'Acknowledged', 'Resolved', 'Suppressed', 'All']);
	});

	it('reads the view from the URL and starts on what is waiting', () => {
		expect(tabOf('sent').key).toBe('sent');
		expect(tabOf('all').key).toBe('all');
		expect(tabOf(null).key).toBe('waiting');
		expect(tabOf('nonsense').key).toBe('waiting');
	});

	it('asks the API for the states of a view', () => {
		expect(stateQuery(tabOf('waiting'))).toBe('state=grouping,held&');
		expect(stateQuery(tabOf('sent'))).toBe('state=sent&');
		expect(stateQuery(tabOf('all'))).toBe('');
	});
});

describe('stateWords', () => {
	it('says when a group that is still collecting will be sent', () => {
		expect(stateWords(row('grouping', { next_timer_at: NOW + 12_000 }), NOW)).toEqual({
			state: 'collecting',
			reason: '',
			when: 'sends in 12 s'
		});
	});

	it('says why and until when a notification is held', () => {
		const held = row('held', {
			reason: 'quiet hours until 07:00 (Europe/Warsaw)',
			held_until: new Date(2026, 9, 14, 7, 0, 0).getTime()
		});
		expect(stateWords(held, NOW)).toEqual({
			state: 'held',
			reason: 'quiet hours until 07:00 (Europe/Warsaw)',
			when: 'until Oct 14 07:00'
		});
		const today = row('held', { reason: 'rate limit', held_until: new Date(2026, 9, 13, 12, 5, 0).getTime() });
		expect(stateWords(today, NOW).when).toBe('until 12:05');
	});

	it('puts the state first and what follows it after, so the two read as one phrase', () => {
		const words = stateWords(row('sent', { sent_count: 1, sent_at: NOW - 240_000 }), NOW);
		expect(`${words.state} ${words.when}`).toBe('sent 4 min ago');
	});

	it('says how long ago the message was sent, whatever changed since', () => {
		expect(stateWords(row('sent', { sent_count: 1, event_count: 4, sent_at: NOW - 240_000 }), NOW).when).toBe('4 min ago');
	});

	it('says how many messages were sent and when the last one went out', () => {
		expect(stateWords(row('sent', { sent_count: 2, sent_at: NOW - 60_000 }), NOW).when).toBe('2 times, last 1 min ago');
	});

	it('falls back on the last change when the send time is not known', () => {
		expect(stateWords(row('sent', { sent_count: 1 }), NOW).when).toBe('3 min ago');
	});

	it('says when the next step of a sent notification is due, without guessing what it is', () => {
		// The row does not say whether the timer repeats the message or escalates it.
		const timed = row('sent', { sent_count: 1, sent_at: NOW - 240_000, next_timer_at: NOW + 240_000 });
		expect(stateWords(timed, NOW).when).toBe('4 min ago, next step in 4 min');
	});

	it('says who acknowledged it and when', () => {
		const acked = row('acknowledged', { acked_by: 'kris@example.com', acked_at: NOW - 120_000 });
		expect(stateWords(acked, NOW)).toEqual({
			state: 'acknowledged',
			reason: '',
			when: 'by kris@… 2 min ago'
		});
	});

	it('says when it was resolved', () => {
		expect(stateWords(row('resolved', { resolved_at: NOW - 3_600_000 }), NOW).when).toBe('1 h ago');
	});

	it('says why a suppressed one was not sent', () => {
		expect(stateWords(row('suppressed', { reason: 'silenced: deploy' }), NOW)).toEqual({
			state: 'not sent',
			reason: 'silenced: deploy',
			when: '10 min ago'
		});
	});
});

describe('shortWho', () => {
	it('keeps the name of an email address and drops the domain', () => {
		expect(shortWho('kris@example.com')).toBe('kris@…');
		expect(shortWho('API token')).toBe('API token');
		expect(shortWho(null)).toBe('someone');
	});
});

describe('actions', () => {
	it('lets what was sent or is waiting be acknowledged', () => {
		const can = (['grouping', 'held', 'sent', 'acknowledged', 'resolved', 'suppressed'] as const).filter(canAcknowledge);
		expect(can).toEqual(['grouping', 'held', 'sent']);
	});

	it('lets anything open be resolved', () => {
		const can = (['grouping', 'held', 'sent', 'acknowledged', 'resolved', 'suppressed'] as const).filter(canResolve);
		expect(can).toEqual(['grouping', 'held', 'sent', 'acknowledged']);
	});
});

describe('roleWords', () => {
	it('says nothing for the event that opened the notification', () => {
		expect(roleWords('first', 3)).toBe('');
	});

	it('says how a later event relates to its notification', () => {
		expect(roleWords('duplicate', 2)).toBe('counted as a duplicate of an earlier event');
		expect(roleWords('grouped', 4)).toBe('joined a group of 4 events');
		expect(roleWords('resolved', 2)).toBe('resolved this notification');
	});
});

describe('journeyWords', () => {
	it('says nothing about a notification that was simply sent', () => {
		expect(journeyWords({ state: 'sent' }, NOW)).toEqual({ text: '', reason: '' });
		expect(journeyWords({ state: 'acknowledged' }, NOW)).toEqual({ text: '', reason: '' });
	});

	it('says why a suppressed one was not sent', () => {
		expect(journeyWords({ state: 'suppressed', reason: 'silenced: deploy' }, NOW)).toEqual({
			text: 'Not sent',
			reason: 'silenced: deploy'
		});
	});

	it('says until when a held one waits', () => {
		const held_until = new Date(2026, 9, 13, 12, 5, 0).getTime();
		expect(journeyWords({ state: 'held', reason: 'rate limit', held_until }, NOW)).toEqual({
			text: 'Held until 12:05',
			reason: 'rate limit'
		});
		expect(journeyWords({ state: 'held' }, NOW).text).toBe('Held');
	});

	it('says that a group is still collecting', () => {
		expect(journeyWords({ state: 'grouping' }, NOW).text).toBe('Collecting events to send as one message');
	});
});
