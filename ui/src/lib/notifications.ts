import type { NotificationRow, NotificationState } from './api/types.ts';
import { ago, timeOfDay, until } from './format.ts';

/** The views of the notifications page. A view without states shows every notification. */
export const TABS = [
	{ key: 'waiting', label: 'Waiting', states: ['grouping', 'held'] },
	{ key: 'sent', label: 'Sent', states: ['sent'] },
	{ key: 'acknowledged', label: 'Acknowledged', states: ['acknowledged'] },
	{ key: 'resolved', label: 'Resolved', states: ['resolved'] },
	{ key: 'suppressed', label: 'Not sent', states: ['suppressed'] },
	{ key: 'all', label: 'All', states: [] }
] as const;

export type Tab = (typeof TABS)[number];

/** The view named in the URL. The page starts on what is waiting. */
export function tabOf(param: string | null): Tab {
	return TABS.find((tab) => tab.key === param) ?? TABS[0];
}

/** The start of the API query for a view: its states, ready for more parameters. */
export function stateQuery(tab: Tab): string {
	return tab.states.length > 0 ? `state=${tab.states.join(',')}&` : '';
}

/** Who did something, short enough for a row: an email address loses its domain. */
export function shortWho(who: string | null | undefined): string {
	if (!who) return 'someone';
	const at = who.indexOf('@');
	return at > 0 ? `${who.slice(0, at)}@…` : who;
}

function sentWords(row: NotificationRow, now: number): string {
	const last = ago(row.sent_at ?? row.updated_at, now);
	const sent = row.sent_count > 1 ? `${row.sent_count} times, last ${last}` : last;
	// The timer is a repeat of the message or the next escalation step; a row does not say which.
	return row.next_timer_at ? `${sent}, next step ${until(row.next_timer_at, now)}` : sent;
}

/**
 * A notification's state in words, the time that matters for it, and why it is in that state.
 * `state` and `when` read as one phrase: "sent" "3 min ago", "held" "until 07:00".
 */
export function stateWords(row: NotificationRow, now: number): { state: string; reason: string; when: string } {
	const reason = row.reason ?? '';
	switch (row.state) {
		case 'grouping':
			return { state: 'collecting', reason: '', when: row.next_timer_at ? `sends ${until(row.next_timer_at, now)}` : '' };
		case 'held':
			return { state: 'held', reason, when: row.held_until ? `until ${timeOfDay(row.held_until, now)}` : '' };
		case 'sent':
			return { state: 'sent', reason: '', when: sentWords(row, now) };
		case 'acknowledged':
			return {
				state: 'acknowledged',
				reason: '',
				when: `by ${shortWho(row.acked_by)} ${ago(row.acked_at ?? row.updated_at, now)}`
			};
		case 'resolved':
			return { state: 'resolved', reason: '', when: ago(row.resolved_at ?? row.updated_at, now) };
		case 'suppressed':
			return { state: 'not sent', reason, when: ago(row.created_at, now) };
	}
}

/** Acknowledging stops reminders, and stops a waiting notification from being sent. */
export function canAcknowledge(state: NotificationState): boolean {
	return state === 'grouping' || state === 'held' || state === 'sent';
}

/** Anything still open can be closed by hand. */
export function canResolve(state: NotificationState): boolean {
	return state !== 'resolved' && state !== 'suppressed';
}

/** How an event relates to a notification it did not open. Empty for the event that opened it. */
export function roleWords(role: string, eventCount: number): string {
	if (role === 'duplicate') return 'counted as a duplicate of an earlier event';
	if (role === 'grouped') return `joined a group of ${eventCount} events`;
	if (role === 'resolved') return 'resolved this notification';
	return '';
}

/** What an event's page says about a notification that has not simply been sent, and why. */
export function journeyWords(
	notification: { state: string; reason?: string | null; held_until?: number | null },
	now: number
): { text: string; reason: string } {
	const reason = notification.reason ?? '';
	if (notification.state === 'suppressed') return { text: 'Not sent', reason };
	if (notification.state === 'held') {
		const until = notification.held_until;
		return { text: until ? `Held until ${timeOfDay(until, now)}` : 'Held', reason };
	}
	if (notification.state === 'grouping') return { text: 'Collecting events to send as one message', reason: '' };
	return { text: '', reason: '' };
}
