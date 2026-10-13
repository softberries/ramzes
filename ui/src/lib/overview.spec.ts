import { describe, expect, it } from 'vitest';
import {
	PERIODS,
	bucketLabel,
	compact,
	emptyBucket,
	fillBuckets,
	labelSide,
	niceMax,
	peakIndex,
	periodOf,
	proportion,
	shareWords,
	trend
} from './overview.ts';

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** A bucket with that many events and nothing else. */
const bucket = (start: number, events: number) => ({ ...emptyBucket(start), events });

describe('periods', () => {
	it('offers a day, a week and a month', () => {
		expect(PERIODS).toEqual([
			{ hours: 24, label: '24 hours' },
			{ hours: 168, label: '7 days' },
			{ hours: 720, label: '30 days' }
		]);
	});

	it('reads the period from the URL and falls back to a day', () => {
		expect(periodOf('168')).toBe(168);
		expect(periodOf('720')).toBe(720);
		expect(periodOf(null)).toBe(24);
		expect(periodOf('5')).toBe(24);
		expect(periodOf('week')).toBe(24);
	});
});

describe('fillBuckets', () => {
	it('adds the buckets without events as empty ones, oldest first, up to now', () => {
		const since = 100 * HOUR;
		const found = [
			bucket(101 * HOUR, 4),
			bucket(103 * HOUR, 9)
		];
		expect(fillBuckets(found, since, HOUR, 103 * HOUR + 1234)).toEqual([
			bucket(100 * HOUR, 0),
			bucket(101 * HOUR, 4),
			bucket(102 * HOUR, 0),
			bucket(103 * HOUR, 9)
		]);
	});

	it('gives only empty buckets when nothing arrived', () => {
		const filled = fillBuckets([], 10 * DAY, DAY, 12 * DAY + 5);
		expect(filled).toEqual([
			bucket(10 * DAY, 0),
			bucket(11 * DAY, 0),
			bucket(12 * DAY, 0)
		]);
	});

	it('keeps a bucket the server counted after the moment the page read the clock', () => {
		const filled = fillBuckets([bucket(3 * HOUR, 1)], 2 * HOUR, HOUR, 2 * HOUR + 10);
		expect(filled.map((b) => b.events)).toEqual([0, 1]);
	});
});

describe('peakIndex', () => {
	it('finds the first of the fullest buckets', () => {
		const buckets = [3, 9, 2, 9].map((events, i) => bucket(i, events));
		expect(peakIndex(buckets)).toBe(1);
	});

	it('finds none when every bucket is empty', () => {
		expect(peakIndex([bucket(0, 0)])).toBeNull();
		expect(peakIndex([])).toBeNull();
	});
});

describe('shareWords', () => {
	it('writes a part of a total as a percentage', () => {
		expect(shareWords(97, 100)).toBe('97%');
		expect(shareWords(1, 3)).toBe('33.3%');
		expect(shareWords(5, 5)).toBe('100%');
		expect(shareWords(0, 5)).toBe('0%');
	});

	it('never rounds a part that is not everything to 100%, or one that is something to 0%', () => {
		expect(shareWords(9999, 10_000)).toBe('99.9%');
		expect(shareWords(1, 10_000)).toBe('0.1%');
	});

	it('has nothing to say about a total of zero', () => {
		expect(shareWords(0, 0)).toBe('');
	});
});

describe('proportion', () => {
	it('gives the width of a bar as a percentage of the largest value', () => {
		expect(proportion(5, 10)).toBe(50);
		expect(proportion(10, 10)).toBe(100);
		expect(proportion(0, 10)).toBe(0);
		expect(proportion(0, 0)).toBe(0);
	});
});

describe('bucketLabel', () => {
	const now = new Date(2026, 9, 13, 12, 30, 0).getTime();

	it('names an hour of today by its time', () => {
		expect(bucketLabel(new Date(2026, 9, 13, 9, 0, 0).getTime(), HOUR, now)).toBe('09:00');
	});

	it('adds the date to an hour of another day', () => {
		expect(bucketLabel(new Date(2026, 9, 12, 13, 0, 0).getTime(), HOUR, now)).toBe('Oct 12 13:00');
	});

	it('names a day by its date, as the server counts days', () => {
		expect(bucketLabel(Date.UTC(2026, 8, 14), DAY, now)).toBe('Sep 14');
	});
});

describe('labelSide', () => {
	it('keeps a label over a bar near an edge inside the chart', () => {
		expect(labelSide(0, 24)).toBe('start');
		expect(labelSide(2, 24)).toBe('start');
		expect(labelSide(12, 24)).toBe('center');
		expect(labelSide(22, 24)).toBe('end');
		expect(labelSide(23, 24)).toBe('end');
	});

	it('centres the label of the only bar', () => {
		expect(labelSide(0, 1)).toBe('center');
	});
});

describe('chart helpers', () => {
	it('tops a scale at the next round number', () => {
		expect([0, 1, 3, 9, 10, 23, 180, 501].map(niceMax)).toEqual([2, 2, 4, 10, 10, 30, 200, 600]);
	});

	it('shortens large counts', () => {
		expect([0, 950, 1234, 34_100, 2_540_000].map(compact)).toEqual(['0', '950', '1.2k', '34k', '2.5M']);
	});

	it('compares a number with the one before it', () => {
		expect(trend(120, 100)).toEqual({ direction: 'up', change: '20%' });
		expect(trend(93, 100)).toEqual({ direction: 'down', change: '7%' });
		expect(trend(100, 100)).toEqual({ direction: 'same', change: '0%' });
		// Nothing before: there is no percentage to give.
		expect(trend(5, 0)).toEqual({ direction: 'up', change: null });
		expect(trend(0, 0)).toEqual({ direction: 'same', change: null });
	});
});
