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
}

/** The one-word outcome of an event from its delivery counts. */
export function outcomeOf(row: DeliveryCounts): Outcome {
	if (!row.routed) return 'pending';
	if (row.deliveries_failed > 0) return 'failed';
	if (row.deliveries_total === 0) return row.route_count === 0 ? 'unrouted' : 'pending';
	return row.deliveries_delivered === row.deliveries_total ? 'delivered' : 'pending';
}
