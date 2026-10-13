import type { LabelCond, Route, RouteMatch } from './api/types.ts';

/** A route's conditions in plain words, for lists. */
export function summarize(match: RouteMatch, sourceNames: Record<string, string>): string {
	const parts: string[] = [];
	if (match.topic) parts.push(`topic ${match.topic}`);
	if (match.min_severity) {
		parts.push(match.min_severity === 'critical' ? 'severity critical' : `severity ${match.min_severity} or higher`);
	}
	if (match.source_id) parts.push(`from ${sourceNames[match.source_id] ?? 'a deleted source'}`);
	for (const cond of match.labels ?? []) {
		if (cond.op === 'equals') parts.push(`${cond.key} = ${cond.value}`);
		else if (cond.op === 'matches') parts.push(`${cond.key} matches ${cond.pattern}`);
		else parts.push(`has label ${cond.key}`);
	}
	return parts.length > 0 ? parts.join(', ') : 'Every event';
}

function cleanCond(cond: LabelCond): LabelCond | null {
	const key = cond.key.trim();
	if (key === '') return null;
	if (cond.op === 'equals') return { op: 'equals', key, value: cond.value };
	if (cond.op === 'matches') return cond.pattern === '' ? null : { op: 'matches', key, pattern: cond.pattern };
	return { op: 'exists', key };
}

/** The match to send to the API: empty fields and unfinished label conditions are left out. */
export function cleanMatch(draft: RouteMatch): RouteMatch {
	const match: RouteMatch = {};
	const topic = draft.topic?.trim();
	if (topic) match.topic = topic;
	if (draft.min_severity) match.min_severity = draft.min_severity;
	if (draft.source_id) match.source_id = draft.source_id;
	const labels = (draft.labels ?? []).map(cleanCond).filter((cond) => cond !== null);
	if (labels.length > 0) match.labels = labels;
	return match;
}

/** The order after moving `id` one place up (-1) or down (+1), or null when it cannot move. */
export function moved(ids: string[], id: string, step: 1 | -1): string[] | null {
	const from = ids.indexOf(id);
	const to = from + step;
	if (from < 0 || to < 0 || to >= ids.length) return null;
	const next = [...ids];
	[next[from], next[to]] = [next[to], next[from]];
	return next;
}

/** The routes that send to a connection. */
export function routesUsing(routes: Route[], connectionId: string): Route[] {
	return routes.filter((route) => route.targets.some((target) => target.connection_id === connectionId));
}
