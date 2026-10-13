import { describe, expect, it } from 'vitest';
import type { EventRow } from './api/types';
import { filtersFromQuery, filtersToQuery, isFiltered, mergeNewest, moveSelection, type FeedFilters } from './feed';

function row(id: string, delivered = 0): EventRow {
	return {
		id,
		workspace_id: 'w',
		source_id: 's',
		topic: 'orders.failed',
		severity: 'error',
		status: 'firing',
		title: `event ${id}`,
		body: null,
		labels: {},
		payload: null,
		dedup_key: null,
		occurred_at: 1,
		received_at: 1,
		routed: true,
		route_count: 1,
		deliveries_total: 1,
		deliveries_delivered: delivered,
		deliveries_failed: 0,
		targets: []
	};
}

describe('filters and the query string', () => {
	const full: FeedFilters = {
		source: 'src-1',
		topic: 'orders.*',
		min_severity: 'error',
		label: 'env=prod',
		q: 'charge failed',
		outcome: 'failed',
		since: 1791884467000,
		until: 1791884468000
	};

	it('round-trips every filter', () => {
		expect(filtersFromQuery(filtersToQuery(full))).toEqual(full);
	});

	it('writes nothing for no filters and skips empty values', () => {
		expect(filtersToQuery({})).toBe('');
		expect(filtersToQuery({ topic: '', q: '  ', source: undefined })).toBe('');
	});

	it('encodes values that need it', () => {
		expect(filtersToQuery({ label: 'env=prod', q: 'a b&c' })).toBe('label=env%3Dprod&q=a+b%26c');
	});

	it('ignores values that are not valid', () => {
		expect(filtersFromQuery('min_severity=loud&outcome=maybe&since=yesterday&label=novalue&topic=a')).toEqual({
			topic: 'a'
		});
	});

	it('knows whether anything is filtered', () => {
		expect(isFiltered({})).toBe(false);
		expect(isFiltered({ q: '' })).toBe(false);
		expect(isFiltered({ outcome: 'failed' })).toBe(true);
	});
});

describe('mergeNewest', () => {
	it('puts unseen newer rows on top, newest first', () => {
		const merged = mergeNewest([row('03'), row('02'), row('01')], [row('05'), row('04'), row('03')], 3);
		expect(merged.rows.map((r) => r.id)).toEqual(['05', '04', '03', '02', '01']);
		expect(merged.added).toBe(2);
	});

	it('refreshes rows already shown with their newer state', () => {
		const merged = mergeNewest([row('02', 0), row('01', 0)], [row('02', 1), row('01', 0)], 2);
		expect(merged.added).toBe(0);
		expect(merged.rows[0].deliveries_delivered).toBe(1);
	});

	it('keeps older loaded rows that a full fresh page does not reach', () => {
		const merged = mergeNewest([row('03'), row('02'), row('01')], [row('04'), row('03')], 2);
		expect(merged.rows.map((r) => r.id)).toEqual(['04', '03', '02', '01']);
	});

	it('drops a shown row the server no longer returns within the fresh page', () => {
		// 03 stopped matching the filter (for example it was delivered while filtering on pending).
		const merged = mergeNewest([row('04'), row('03'), row('02'), row('01')], [row('05'), row('04'), row('02')], 3);
		expect(merged.rows.map((r) => r.id)).toEqual(['05', '04', '02', '01']);
		expect(merged.added).toBe(1);
	});

	it('shows exactly the fresh rows when they are the whole result', () => {
		// Fewer rows than a page means there is nothing older on the server.
		const merged = mergeNewest([row('03'), row('02'), row('01')], [row('03'), row('01')], 50);
		expect(merged.rows.map((r) => r.id)).toEqual(['03', '01']);
		expect(mergeNewest([row('01')], [], 50).rows).toEqual([]);
		expect(mergeNewest([], [row('01')], 50).rows.map((r) => r.id)).toEqual(['01']);
	});
});

describe('moveSelection', () => {
	const rows = [row('03'), row('02'), row('01')];

	it('starts at the first row and moves within bounds', () => {
		expect(moveSelection(rows, null, 1)).toBe('03');
		expect(moveSelection(rows, '03', 1)).toBe('02');
		expect(moveSelection(rows, '01', 1)).toBe('01');
		expect(moveSelection(rows, '03', -1)).toBe('03');
	});

	it('recovers when the selected row is gone or nothing is listed', () => {
		expect(moveSelection(rows, 'gone', 1)).toBe('03');
		expect(moveSelection([], null, 1)).toBeNull();
	});
});
