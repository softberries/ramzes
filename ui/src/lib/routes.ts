import type { LabelCond, NewEvent, Route, RouteInput, RouteMatch, RouteResult, Target } from './api/types.ts';

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
	if (match.expr) parts.push(`where ${match.expr}`);
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
	const expr = draft.expr?.trim();
	if (expr) match.expr = expr;
	return match;
}

/**
 * The targets to send to the API: rows without a connection are left out, and so are an empty
 * template and a destination that is empty or that the connection's connector does not take.
 */
export function cleanTargets(draft: Target[], takesDestination: (connectionId: string) => boolean): Target[] {
	return draft
		.filter((target) => target.connection_id !== '')
		.map((target) => {
			const clean: Target = { connection_id: target.connection_id };
			const destination = target.destination?.trim();
			if (destination && takesDestination(target.connection_id)) clean.destination = destination;
			if (target.template_id) clean.template_id = target.template_id;
			return clean;
		});
}

/** A stored route as the API takes it back, for saving it with one thing changed. */
export function inputOfRoute(route: Route): RouteInput {
	return {
		name: route.name,
		enabled: route.enabled,
		stop: route.stop,
		match: route.match,
		targets: route.targets,
		...(route.shaping ? { shaping: route.shaping } : {}),
		...(route.escalation ? { escalation: route.escalation } : {})
	};
}

/**
 * The routes to try an event against: the stored routes in list order, with the route being edited
 * replaced by its unsaved draft, or a new draft added at the end.
 */
export function routesForTest(routes: Route[], draft: { id: string | null; input: RouteInput } | null): RouteInput[] {
	const named = draft && { ...draft.input, name: draft.input.name.trim() || 'New route' };
	const sent: RouteInput[] = routes.map((route) => (named && route.id === draft.id ? named : inputOfRoute(route)));
	if (named && draft.id === null) sent.push(named);
	return sent;
}

/** A valid event to start a test from. */
export const SAMPLE_EVENT = JSON.stringify(
	{
		topic: 'orders.payment.failed',
		title: 'Payment failed for order 1042',
		severity: 'error',
		labels: { env: 'prod', service: 'checkout' },
		payload: { amount: 250, currency: 'EUR' }
	},
	null,
	2
);

/** Reads an event written as JSON, or says why it cannot be used. */
export function parseSampleEvent(text: string): { event?: NewEvent; problem?: string } {
	let value: unknown;
	try {
		value = JSON.parse(text);
	} catch {
		return { problem: 'The sample event is not valid JSON.' };
	}
	const event = value as Partial<NewEvent> | null;
	if (typeof event !== 'object' || event === null || typeof event.topic !== 'string' || typeof event.title !== 'string') {
		return { problem: 'The sample event must be a JSON object with a topic and a title.' };
	}
	return { event: event as NewEvent };
}

/** What the tester found for one route, in plain words. */
export function outcomeWords(result: RouteResult): string {
	if (result.outcome === 'matched') return 'matched';
	if (result.outcome === 'closes_only') {
		return result.targets.length > 0
			? 'too mild to match, but it resolves what this route opened'
			: 'not matched: too mild, and nothing of this route is open';
	}
	if (result.outcome === 'not_matched') return 'not matched';
	if (result.outcome === 'disabled') return 'off';
	if (result.outcome === 'skipped') return 'skipped, an earlier route stops here';
	return `expression failed: ${result.error ?? 'no reason was given'}`;
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
