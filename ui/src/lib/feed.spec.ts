import { describe, expect, it } from 'vitest';
import type { EventRow } from './api/types';
import {
	DEFAULT_VIEW,
	filtersFromQuery,
	filtersToQuery,
	isFiltered,
	isLiveView,
	moveSelection,
	viewFromQuery,
	viewToApi,
	viewToQuery,
	type FeedFilters
} from './feed';

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
		notifications_waiting: 0,
		notifications_suppressed: 0,
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

describe('the view of the feed', () => {
	it('is the first page of the newest events unless the URL says otherwise', () => {
		expect(viewFromQuery('')).toEqual(DEFAULT_VIEW);
		expect(viewFromQuery('page=0&size=7&sort=colour&order=sideways')).toEqual(DEFAULT_VIEW);
		expect(viewFromQuery('page=3&size=100&sort=severity&order=asc')).toEqual({
			page: 3,
			size: 100,
			sort: 'severity',
			descending: false
		});
	});

	it('goes into the URL only where it differs from the default', () => {
		expect(viewToQuery(DEFAULT_VIEW)).toBe('');
		expect(viewToQuery({ page: 2, size: 25, sort: 'topic', descending: false })).toBe(
			'sort=topic&order=asc&page=2&size=25'
		);
		expect(viewFromQuery(viewToQuery({ page: 4, size: 25, sort: 'title', descending: true }))).toEqual({
			page: 4,
			size: 25,
			sort: 'title',
			descending: true
		});
	});

	it('asks the API for a page by its offset', () => {
		expect(viewToApi(DEFAULT_VIEW)).toBe('limit=50');
		expect(viewToApi({ page: 3, size: 25, sort: 'severity', descending: false })).toBe(
			'limit=25&sort=severity&order=asc&offset=50'
		);
	});

	it('shows arrivals only where they would appear at the top', () => {
		expect(isLiveView(DEFAULT_VIEW)).toBe(true);
		expect(isLiveView({ ...DEFAULT_VIEW, page: 2 })).toBe(false);
		expect(isLiveView({ ...DEFAULT_VIEW, sort: 'topic' })).toBe(false);
		expect(isLiveView({ ...DEFAULT_VIEW, descending: false })).toBe(false);
	});
});
