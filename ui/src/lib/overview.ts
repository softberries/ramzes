import type { Bucket } from './api/types.ts';
import { timeOfDay } from './format.ts';

const DAY_MS = 86_400_000;
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** The periods the overview can cover. */
export const PERIODS = [
	{ hours: 24, label: '24 hours' },
	{ hours: 168, label: '7 days' },
	{ hours: 720, label: '30 days' }
];

/** The period named in the URL, in hours. A day when there is none or it is not one of the choices. */
export function periodOf(param: string | null): number {
	return PERIODS.find((period) => String(period.hours) === param)?.hours ?? PERIODS[0].hours;
}

/** A bucket in which nothing happened. */
export function emptyBucket(start: number): Bucket {
	return { start, events: 0, critical: 0, error: 0, warning: 0, info: 0, debug: 0, delivered: 0, failed: 0 };
}

/**
 * Every bucket of the period, oldest first. The server leaves out buckets in which nothing happened;
 * they are added here as empty ones, from the start of the period to the bucket `now` falls in.
 */
export function fillBuckets(found: Bucket[], since: number, bucketMs: number, now: number): Bucket[] {
	const known: Record<number, Bucket> = {};
	for (const bucket of found) known[bucket.start] = bucket;
	const last = Math.max(now, ...found.map((bucket) => bucket.start));
	const filled: Bucket[] = [];
	for (let start = since; start <= last; start += bucketMs) filled.push(known[start] ?? emptyBucket(start));
	return filled;
}

/** Where the fullest bucket is; the first one when several are equally full, null when all are empty. */
export function peakIndex(buckets: Bucket[]): number | null {
	let peak: number | null = null;
	buckets.forEach((bucket, index) => {
		if (bucket.events > (peak === null ? 0 : buckets[peak].events)) peak = index;
	});
	return peak;
}

/**
 * A part of a total as a percentage with at most one decimal. Only everything is 100% and only
 * nothing is 0%. Empty when the total is zero.
 */
export function shareWords(part: number, total: number): string {
	if (total <= 0) return '';
	if (part >= total) return '100%';
	if (part <= 0) return '0%';
	const tenths = Math.min(Math.max(Math.round((part / total) * 1000), 1), 999);
	return `${tenths % 10 === 0 ? tenths / 10 : (tenths / 10).toFixed(1)}%`;
}

/** A value as a percentage of the largest one, for the length of a bar. */
export function proportion(value: number, max: number): number {
	return max > 0 ? (value / max) * 100 : 0;
}

/** Which side of its bar a label hangs from, so that one near an edge of the chart stays inside it. */
export function labelSide(index: number, count: number): 'start' | 'center' | 'end' {
	if (count < 2) return 'center';
	const place = index / (count - 1);
	return place < 0.15 ? 'start' : place > 0.85 ? 'end' : 'center';
}

/**
 * What to call a bucket. An hour is named by its local time. A day is named by its date in UTC,
 * because that is where the server cuts days.
 */
export function bucketLabel(start: number, bucketMs: number, now: number): string {
	if (bucketMs < DAY_MS) return timeOfDay(start, now);
	const day = new Date(start);
	return `${MONTHS[day.getUTCMonth()]} ${day.getUTCDate()}`;
}

/**
 * The top of a chart's scale: the smallest round number the largest value fits under, chosen so
 * that half of it, which the scale also names, is a whole number.
 */
export function niceMax(largest: number): number {
	if (largest <= 2) return 2;
	const magnitude = 10 ** Math.floor(Math.log10(largest));
	const steps = magnitude === 1 ? [2, 4, 6, 8, 10] : [1, 2, 3, 4, 5, 6, 8, 10];
	return steps.map((step) => step * magnitude).find((top) => top >= largest) ?? 10 * magnitude;
}

/** A count in few characters, for an axis or a small label: 950, 1.2k, 34k, 2.5M. */
export function compact(count: number): string {
	if (count < 1000) return String(count);
	const [value, unit] = count < 1_000_000 ? [count / 1000, 'k'] : [count / 1_000_000, 'M'];
	return `${value < 10 ? Math.round(value * 10) / 10 : Math.round(value)}${unit}`;
}

/** How a number compares with the one before it. `change` is null when there was nothing to compare with. */
export function trend(current: number, previous: number): { direction: 'up' | 'down' | 'same'; change: string | null } {
	if (previous <= 0) return { direction: current > 0 ? 'up' : 'same', change: null };
	if (current === previous) return { direction: 'same', change: '0%' };
	const percent = Math.abs(((current - previous) / previous) * 100);
	const shown = percent >= 999.5 ? '999+' : percent < 10 ? String(Math.round(percent * 10) / 10) : String(Math.round(percent));
	return { direction: current > previous ? 'up' : 'down', change: `${shown}%` };
}
