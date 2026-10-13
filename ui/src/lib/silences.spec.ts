import { describe, expect, it } from 'vitest';
import type { SilenceRow } from './api/types.ts';
import {
	DURATIONS,
	groupSilences,
	localInput,
	matchFromQuery,
	newSilence,
	silenceQuery,
	silenceWhen
} from './silences.ts';

const NOW = new Date(2026, 9, 13, 12, 0, 0).getTime();
const HOUR = 3_600_000;

function silence(id: string, starts_at: number, ends_at: number): SilenceRow {
	return { id, match: { topic: 'orders.*' }, starts_at, ends_at, author: 'kris@example.com', created_at: starts_at };
}

describe('groupSilences', () => {
	it('sorts silences into active, upcoming and ended', () => {
		const groups = groupSilences(
			[
				silence('later', NOW + HOUR, NOW + 2 * HOUR),
				silence('long', NOW - HOUR, NOW + 5 * HOUR),
				silence('short', NOW - HOUR, NOW + HOUR),
				silence('old', NOW - 3 * HOUR, NOW - 2 * HOUR),
				silence('just', NOW - 2 * HOUR, NOW)
			],
			NOW
		);
		expect(groups.active.map((s) => s.id)).toEqual(['short', 'long']);
		expect(groups.upcoming.map((s) => s.id)).toEqual(['later']);
		expect(groups.ended.map((s) => s.id)).toEqual(['just', 'old']);
	});
});

describe('silenceWhen', () => {
	it('says when an active silence ends', () => {
		expect(silenceWhen(silence('a', NOW - HOUR, NOW + 25 * 60_000), NOW)).toBe('ends in 25 min, at 12:25');
	});

	it('says when an upcoming one starts and how long it lasts', () => {
		expect(silenceWhen(silence('a', NOW + 2 * HOUR, NOW + 3 * HOUR), NOW)).toBe('starts in 2 h, at 14:00, for 1 h');
	});

	it('says when an ended one ended', () => {
		expect(silenceWhen(silence('a', NOW - 5 * HOUR, NOW - 3 * HOUR), NOW)).toBe('ended 3 h ago');
	});
});

describe('prefill from the URL', () => {
	it('writes a topic and labels as a query and reads them back as conditions', () => {
		const query = silenceQuery({ topic: 'orders.failed', labels: { env: 'prod', service: 'pay=ments' } });
		expect(query).toBe('topic=orders.failed&label=env%3Dprod&label=service%3Dpay%3Dments');
		expect(matchFromQuery(new URLSearchParams(query))).toEqual({
			topic: 'orders.failed',
			labels: [
				{ op: 'equals', key: 'env', value: 'prod' },
				{ op: 'equals', key: 'service', value: 'pay=ments' }
			]
		});
	});

	it('reads nothing from an empty or malformed query', () => {
		expect(matchFromQuery(new URLSearchParams(''))).toEqual({});
		expect(matchFromQuery(new URLSearchParams('label=novalue&label==x&topic=%20'))).toEqual({});
	});
});

describe('newSilence', () => {
	it('offers the four quick lengths', () => {
		expect(DURATIONS.map((d) => d.label)).toEqual(['30 min', '1 h', '4 h', '24 h']);
		expect(DURATIONS.map((d) => d.seconds)).toEqual([1800, 3600, 14_400, 86_400]);
	});

	it('builds a silence of a fixed length with a cleaned match and comment', () => {
		const made = newSilence({ topic: ' orders.* ', labels: [{ op: 'equals', key: '', value: '' }] }, { seconds: 3600 }, ' deploy ', NOW);
		expect(made).toEqual({ body: { match: { topic: 'orders.*' }, duration_s: 3600, comment: 'deploy' } });
	});

	it('leaves out an empty comment', () => {
		expect(newSilence({ topic: 'a' }, { seconds: 1800 }, '  ', NOW)).toEqual({
			body: { match: { topic: 'a' }, duration_s: 1800 }
		});
	});

	it('builds a silence that lasts until a local date and time', () => {
		const made = newSilence({ topic: 'a' }, { until: '2026-10-14T07:00' }, '', NOW);
		expect(made).toEqual({ body: { match: { topic: 'a' }, ends_at: new Date(2026, 9, 14, 7, 0, 0).getTime() } });
	});

	it('refuses a silence without conditions', () => {
		const made = newSilence({ topic: '  ', labels: [] }, { seconds: 3600 }, '', NOW);
		expect(made).toEqual({
			problem: 'Add at least one condition. A silence without conditions would silence every event.'
		});
	});

	it('refuses an end that is missing or not in the future', () => {
		expect(newSilence({ topic: 'a' }, { until: '' }, '', NOW)).toEqual({
			problem: 'Choose the date and time the silence ends.'
		});
		expect(newSilence({ topic: 'a' }, { until: '2026-10-13T11:59' }, '', NOW)).toEqual({
			problem: 'The silence must end in the future.'
		});
	});
});

describe('localInput', () => {
	it('writes a time the way a date-and-time field takes it, in local time', () => {
		expect(localInput(new Date(2026, 0, 5, 7, 3, 59).getTime())).toBe('2026-01-05T07:03');
		expect(localInput(new Date(2026, 9, 14, 23, 30, 0).getTime())).toBe('2026-10-14T23:30');
	});
});
