import type { LabelCond, NewSilence, RouteMatch, SilenceRow } from './api/types.ts';
import { ago, timeOfDay, until } from './format.ts';
import { cleanMatch } from './routes.ts';
import { spanWords } from './shaping.ts';

/** The quick choices for how long a silence lasts. */
export const DURATIONS = [
	{ label: '30 min', seconds: 1800 },
	{ label: '1 h', seconds: 3600 },
	{ label: '4 h', seconds: 14_400 },
	{ label: '24 h', seconds: 86_400 }
];

/**
 * Silences by whether they apply now, later or no longer. Active ones are listed with the one ending
 * soonest first, upcoming ones with the one starting soonest first, ended ones with the latest first.
 */
export function groupSilences(
	silences: SilenceRow[],
	now: number
): { active: SilenceRow[]; upcoming: SilenceRow[]; ended: SilenceRow[] } {
	return {
		active: silences.filter((s) => s.starts_at <= now && now < s.ends_at).sort((a, b) => a.ends_at - b.ends_at),
		upcoming: silences.filter((s) => s.starts_at > now && now < s.ends_at).sort((a, b) => a.starts_at - b.starts_at),
		ended: silences.filter((s) => s.ends_at <= now).sort((a, b) => b.ends_at - a.ends_at)
	};
}

/** When a silence ends, starts or ended, in words. */
export function silenceWhen(silence: SilenceRow, now: number): string {
	if (silence.ends_at <= now) return `ended ${ago(silence.ends_at, now)}`;
	if (silence.starts_at > now) {
		const length = spanWords(Math.round((silence.ends_at - silence.starts_at) / 1000));
		return `starts ${until(silence.starts_at, now)}, at ${timeOfDay(silence.starts_at, now)}, for ${length}`;
	}
	return `ends ${until(silence.ends_at, now)}, at ${timeOfDay(silence.ends_at, now)}`;
}

/** The query that opens the silence form filled in for events like this one. */
export function silenceQuery(event: { topic: string; labels: Record<string, string> }): string {
	const params = new URLSearchParams({ topic: event.topic });
	for (const [key, value] of Object.entries(event.labels)) params.append('label', `${key}=${value}`);
	return params.toString();
}

/** The conditions a query asks the silence form to start with: `topic`, and `label=key=value` for each label. */
export function matchFromQuery(params: Pick<URLSearchParams, 'get' | 'getAll'>): RouteMatch {
	const match: RouteMatch = {};
	const topic = params.get('topic')?.trim();
	if (topic) match.topic = topic;
	const labels: LabelCond[] = [];
	for (const pair of params.getAll('label')) {
		const at = pair.indexOf('=');
		if (at > 0) labels.push({ op: 'equals', key: pair.slice(0, at), value: pair.slice(at + 1) });
	}
	if (labels.length > 0) match.labels = labels;
	return match;
}

/** A time as a date-and-time field takes it: local, to the minute. */
export function localInput(ms: number): string {
	const at = new Date(ms);
	const two = (n: number) => String(n).padStart(2, '0');
	return `${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())}T${two(at.getHours())}:${two(at.getMinutes())}`;
}

/** How long a new silence lasts: a number of seconds from now, or until a local date and time. */
export type SilenceLength = { seconds: number } | { until: string };

/** The silence to send to the API, or why the form cannot be sent as it is. */
export function newSilence(
	draft: RouteMatch,
	length: SilenceLength,
	comment: string,
	now: number
): { body: NewSilence } | { problem: string } {
	const match = cleanMatch(draft);
	if (Object.keys(match).length === 0) {
		return { problem: 'Add at least one condition. A silence without conditions would silence every event.' };
	}
	const note = comment.trim();
	const rest = note ? { comment: note } : {};
	if ('seconds' in length) return { body: { match, duration_s: length.seconds, ...rest } };
	const ends = length.until ? new Date(length.until).getTime() : NaN;
	if (Number.isNaN(ends)) return { problem: 'Choose the date and time the silence ends.' };
	if (ends <= now) return { problem: 'The silence must end in the future.' };
	return { body: { match, ends_at: ends, ...rest } };
}
