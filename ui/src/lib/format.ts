import type { Outcome } from './api/types';

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

function two(n: number): string {
	return String(n).padStart(2, '0');
}

/** Local time of day with seconds; prefixed with the date when it is not today. */
export function clock(ms: number, now: number = Date.now()): string {
	const at = new Date(ms);
	const time = `${two(at.getHours())}:${two(at.getMinutes())}:${two(at.getSeconds())}`;
	const today = new Date(now);
	const sameDay =
		at.getFullYear() === today.getFullYear() &&
		at.getMonth() === today.getMonth() &&
		at.getDate() === today.getDate();
	return sameDay ? time : `${MONTHS[at.getMonth()]} ${at.getDate()} ${time}`;
}

/** Local hours and minutes; prefixed with the date when it is not today. */
export function timeOfDay(ms: number, now: number = Date.now()): string {
	return clock(ms, now).slice(0, -3);
}

function span(ms: number): string {
	const seconds = Math.floor(ms / 1000);
	if (seconds < 60) return `${seconds} s`;
	const minutes = Math.floor(seconds / 60);
	if (minutes < 60) return `${minutes} min`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours} h`;
	return `${Math.floor(hours / 24)} d`;
}

/** How long ago a time was, in the largest fitting unit. */
export function ago(ms: number, now: number = Date.now()): string {
	const elapsed = now - ms;
	return elapsed < 1000 ? 'just now' : `${span(elapsed)} ago`;
}

/** How far in the future a time is. */
export function until(ms: number, now: number = Date.now()): string {
	const remaining = ms - now;
	return remaining <= 0 ? 'now' : `in ${span(remaining)}`;
}

/** A length of time in its two largest units: "4 min 12 s". As time left, the smaller one can be watched running down. */
export function countdown(ms: number): string {
	const seconds = Math.max(0, Math.floor(ms / 1000));
	const parts: [number, string][] = [
		[Math.floor(seconds / 86_400), 'd'],
		[Math.floor(seconds / 3600) % 24, 'h'],
		[Math.floor(seconds / 60) % 60, 'min'],
		[seconds % 60, 's']
	];
	const first = parts.findIndex(([amount]) => amount > 0);
	if (first < 0) return '0 s';
	return parts
		.slice(first, first + 2)
		.filter(([amount]) => amount > 0)
		.map(([amount, unit]) => `${amount} ${unit}`)
		.join(' ');
}

/** A length of time: milliseconds below one second, seconds above. */
export function duration(ms: number): string {
	return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
}

interface DeliveryCounts {
	routed: boolean;
	route_count: number;
	deliveries_total: number;
	deliveries_delivered: number;
	deliveries_failed: number;
	notifications_waiting: number;
	notifications_suppressed: number;
}

/** The one-word outcome of an event from its delivery and notification counts. */
export function outcomeOf(row: DeliveryCounts): Outcome {
	if (!row.routed) return 'pending';
	if (row.deliveries_failed > 0) return 'failed';
	if (row.route_count === 0) return 'unrouted';
	// Something is still to be sent, whatever else already went out.
	if (row.notifications_waiting > 0) return 'waiting';
	if (row.deliveries_total === 0) return row.notifications_suppressed > 0 ? 'not_sent' : 'pending';
	return row.deliveries_delivered === row.deliveries_total ? 'delivered' : 'pending';
}
