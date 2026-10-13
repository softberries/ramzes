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
 * Rows already shown are refreshed, unseen ones go on top. `added` counts the unseen ones.
 */
export function mergeNewest(current: EventRow[], fresh: EventRow[]): { rows: EventRow[]; added: number } {
	const known = new Set(current.map((row) => row.id));
	const added = fresh.filter((row) => !known.has(row.id)).length;
	const freshIds = new Set(fresh.map((row) => row.id));
	const rows = [...fresh, ...current.filter((row) => !freshIds.has(row.id))];
	// Ids are time-ordered, so sorting by id descending is newest first.
	rows.sort((a, b) => (a.id < b.id ? 1 : a.id > b.id ? -1 : 0));
	return { rows, added };
}

/** The id selected after moving one row down (+1) or up (-1) with the keyboard. */
export function moveSelection(rows: EventRow[], selected: string | null, step: 1 | -1): string | null {
	if (rows.length === 0) return null;
	const at = rows.findIndex((row) => row.id === selected);
	if (at < 0) return rows[0].id;
	return rows[Math.min(Math.max(at + step, 0), rows.length - 1)].id;
}
