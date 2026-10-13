import { OUTCOMES, SEVERITIES, type EventRow, type Outcome, type Severity } from './api/types.ts';

/** The feed's filters, exactly as they appear in the page URL and in the API query. */
export interface FeedFilters {
	source?: string;
	topic?: string;
	min_severity?: Severity;
	/** One label as `key=value`. */
	label?: string;
	/** Text to find in the title. */
	q?: string;
	outcome?: Outcome;
	/** Received at or after, unix milliseconds. */
	since?: number;
	/** Received at or before, unix milliseconds. */
	until?: number;
}

const TEXT_KEYS = ['source', 'topic', 'label', 'q'] as const;

/** The query string for a set of filters, without a leading `?`. Empty values are left out. */
export function filtersToQuery(filters: FeedFilters): string {
	const params = new URLSearchParams();
	const set = (key: string, value: string | number | undefined) => {
		const text = value === undefined ? '' : String(value).trim();
		if (text !== '') params.set(key, text);
	};
	set('source', filters.source);
	set('topic', filters.topic);
	set('min_severity', filters.min_severity);
	set('label', filters.label);
	set('q', filters.q);
	set('outcome', filters.outcome);
	set('since', filters.since);
	set('until', filters.until);
	return params.toString();
}

/** Reads filters from a query string, dropping anything the API would reject. */
export function filtersFromQuery(query: string | { get(name: string): string | null }): FeedFilters {
	const params = typeof query === 'string' ? new URLSearchParams(query) : query;
	const filters: FeedFilters = {};
	for (const key of TEXT_KEYS) {
		const value = params.get(key)?.trim();
		if (!value) continue;
		if (key === 'label' && !/^[^=]+=/.test(value)) continue;
		filters[key] = value;
	}
	const severity = params.get('min_severity') as Severity | null;
	if (severity && SEVERITIES.includes(severity)) filters.min_severity = severity;
	const outcome = params.get('outcome') as Outcome | null;
	if (outcome && OUTCOMES.includes(outcome)) filters.outcome = outcome;
	for (const key of ['since', 'until'] as const) {
		const value = params.get(key);
		if (value && /^\d+$/.test(value)) filters[key] = Number(value);
	}
	return filters;
}

export function isFiltered(filters: FeedFilters): boolean {
	return filtersToQuery(filters) !== '';
}

/**
 * Combines the rows on screen with a freshly loaded first page (both newest first).
 *
 * Within the span the fresh page covers, the server is right: rows it no longer returns are
 * dropped (they stopped matching the filters) and unseen ones are added. Rows older than the
 * fresh page are kept as loaded. A fresh page shorter than `pageSize` is the whole result.
 * `added` counts the rows that were not on screen before.
 */
export function mergeNewest<Row extends { id: string }>(
	current: Row[],
	fresh: Row[],
	pageSize: number
): { rows: Row[]; added: number } {
	const known = new Set(current.map((row) => row.id));
	const added = fresh.filter((row) => !known.has(row.id)).length;
	if (fresh.length < pageSize) return { rows: fresh, added };
	// Ids are time-ordered, so anything below the oldest fresh id is older than the fresh page.
	const oldest = fresh[fresh.length - 1].id;
	return { rows: [...fresh, ...current.filter((row) => row.id < oldest)], added };
}

/** The id selected after moving one row down (+1) or up (-1) with the keyboard. */
export function moveSelection(rows: EventRow[], selected: string | null, step: 1 | -1): string | null {
	if (rows.length === 0) return null;
	const at = rows.findIndex((row) => row.id === selected);
	if (at < 0) return rows[0].id;
	return rows[Math.min(Math.max(at + step, 0), rows.length - 1)].id;
}

/** What the feed can be ordered by; the API's names. */
export const FEED_SORTS = ['received', 'severity', 'topic', 'title'] as const;
export type FeedSort = (typeof FEED_SORTS)[number];
export const FEED_PAGE_SIZES = [25, 50, 100];

/** Which part of the feed is shown, and in what order. */
export interface FeedView {
	/** Counted from 1. */
	page: number;
	size: number;
	sort: FeedSort;
	descending: boolean;
}

export const DEFAULT_VIEW: FeedView = { page: 1, size: 50, sort: 'received', descending: true };

/** Reads the view from a query string; anything missing or not understood is the default. */
export function viewFromQuery(query: string | { get(name: string): string | null }): FeedView {
	const params = typeof query === 'string' ? new URLSearchParams(query) : query;
	const page = Number(params.get('page'));
	const size = Number(params.get('size'));
	const sort = params.get('sort') as FeedSort | null;
	const order = params.get('order');
	return {
		page: Number.isInteger(page) && page >= 1 ? page : DEFAULT_VIEW.page,
		size: FEED_PAGE_SIZES.includes(size) ? size : DEFAULT_VIEW.size,
		sort: sort && FEED_SORTS.includes(sort) ? sort : DEFAULT_VIEW.sort,
		descending: order === 'asc' ? false : order === 'desc' ? true : DEFAULT_VIEW.descending
	};
}

/** The view as it appears in the page URL: only what differs from the default. */
export function viewToQuery(view: FeedView): string {
	const params = new URLSearchParams();
	if (view.sort !== DEFAULT_VIEW.sort) params.set('sort', view.sort);
	if (view.descending !== DEFAULT_VIEW.descending) params.set('order', view.descending ? 'desc' : 'asc');
	if (view.page !== DEFAULT_VIEW.page) params.set('page', String(view.page));
	if (view.size !== DEFAULT_VIEW.size) params.set('size', String(view.size));
	return params.toString();
}

/** The view as the API is asked for it. */
export function viewToApi(view: FeedView): string {
	const params = new URLSearchParams({ limit: String(view.size) });
	if (view.sort !== 'received') params.set('sort', view.sort);
	if (!view.descending) params.set('order', 'asc');
	if (view.page > 1) params.set('offset', String((view.page - 1) * view.size));
	return params.toString();
}

/** Whether rows arrive at the top of what is shown: the first page of the newest-first order. */
export function isLiveView(view: FeedView): boolean {
	return view.page === 1 && view.sort === 'received' && view.descending;
}
