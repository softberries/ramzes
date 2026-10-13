import type { Severity, Shaping } from './api/types.ts';

export type Unit = 's' | 'min' | 'h';

/** A length of time as a form holds it. An empty number field is `null`. */
export interface Span {
	amount: number | null;
	unit: Unit;
}

export const UNITS: { unit: Unit; label: string; seconds: number }[] = [
	{ unit: 's', label: 'seconds', seconds: 1 },
	{ unit: 'min', label: 'minutes', seconds: 60 },
	{ unit: 'h', label: 'hours', seconds: 3600 }
];

export const DAYS = [
	{ key: 'mon', label: 'Mon' },
	{ key: 'tue', label: 'Tue' },
	{ key: 'wed', label: 'Wed' },
	{ key: 'thu', label: 'Thu' },
	{ key: 'fri', label: 'Fri' },
	{ key: 'sat', label: 'Sat' },
	{ key: 'sun', label: 'Sun' }
];

/** Time zones offered in the quiet-hours field. Any IANA name can be typed. */
export const COMMON_ZONES = [
	'UTC',
	'Europe/London',
	'Europe/Berlin',
	'Europe/Paris',
	'Europe/Warsaw',
	'Europe/Kyiv',
	'America/New_York',
	'America/Chicago',
	'America/Denver',
	'America/Los_Angeles',
	'America/Sao_Paulo',
	'Asia/Dubai',
	'Asia/Kolkata',
	'Asia/Singapore',
	'Asia/Tokyo',
	'Australia/Sydney'
];

/** A route's shaping policy as its form holds it: every part has its fields, switched on or off. */
export interface ShapingForm {
	dedup: { on: boolean; window: Span; key: string };
	group: { on: boolean; by: string; wait: Span; repeat: Span };
	quiet: { on: boolean; tz: string; start: string; end: string; days: string[]; bypass: Severity | '' };
	rate: { on: boolean; max: number | null; window: Span };
	notifyResolved: boolean;
}

/** The seconds as an amount of the largest unit that divides them evenly. */
export function splitSeconds(seconds: number): { amount: number; unit: Unit } {
	const fitting = [...UNITS].reverse().find((u) => seconds >= u.seconds && seconds % u.seconds === 0) ?? UNITS[0];
	return { amount: seconds / fitting.seconds, unit: fitting.unit };
}

/** A number of seconds in words: "30 s", "5 min", "2 h". */
export function spanWords(seconds: number): string {
	const { amount, unit } = splitSeconds(seconds);
	return `${amount} ${unit}`;
}

function toSeconds(span: Span): number {
	const per = UNITS.find((u) => u.unit === span.unit)?.seconds ?? 1;
	return Math.round((span.amount ?? 0) * per);
}

/** The form of a route without a policy. `tz` is the time zone quiet hours start in. */
export function emptyShapingForm(tz: string): ShapingForm {
	return {
		dedup: { on: false, window: { amount: 10, unit: 'min' }, key: '' },
		group: { on: false, by: '', wait: { amount: 30, unit: 's' }, repeat: { amount: null, unit: 'h' } },
		quiet: { on: false, tz, start: '22:00', end: '07:00', days: [], bypass: '' },
		rate: { on: false, max: 5, window: { amount: 5, unit: 'min' } },
		notifyResolved: true
	};
}

/** The form showing a stored policy. Parts the policy lacks keep their starting values, switched off. */
export function shapingToForm(shaping: Shaping | undefined, tz: string): ShapingForm {
	const form = emptyShapingForm(tz);
	if (!shaping) return form;
	const { dedup, group, quiet_hours: quiet, rate_limit: rate } = shaping;
	if (dedup) form.dedup = { on: true, window: splitSeconds(dedup.window_s), key: dedup.key ?? '' };
	if (group) {
		form.group = {
			on: true,
			by: (group.by ?? []).join(', '),
			wait: splitSeconds(group.wait_s),
			repeat: group.repeat_s ? splitSeconds(group.repeat_s) : form.group.repeat
		};
	}
	if (quiet) {
		form.quiet = {
			on: true,
			tz: quiet.tz,
			start: quiet.start,
			end: quiet.end,
			days: quiet.days ?? [],
			bypass: quiet.bypass_severity ?? ''
		};
	}
	if (rate) form.rate = { on: true, max: rate.max, window: splitSeconds(rate.window_s) };
	form.notifyResolved = shaping.notify_resolved ?? true;
	return form;
}

/** The policy to send to the API. Parts that are switched off are left out; an untouched form gives `{}`. */
export function formToShaping(form: ShapingForm): Shaping {
	const shaping: Shaping = {};
	if (form.dedup.on) {
		const key = form.dedup.key.trim();
		shaping.dedup = { window_s: toSeconds(form.dedup.window), ...(key ? { key } : {}) };
	}
	if (form.group.on) {
		const by = form.group.by
			.split(',')
			.map((label) => label.trim())
			.filter((label) => label !== '');
		const repeat = form.group.repeat.amount === null ? null : toSeconds(form.group.repeat);
		shaping.group = { by, wait_s: toSeconds(form.group.wait), ...(repeat ? { repeat_s: repeat } : {}) };
	}
	if (form.quiet.on) {
		shaping.quiet_hours = {
			tz: form.quiet.tz.trim(),
			start: form.quiet.start,
			end: form.quiet.end,
			days: DAYS.map((day) => day.key).filter((day) => form.quiet.days.includes(day)),
			...(form.quiet.bypass ? { bypass_severity: form.quiet.bypass } : {})
		};
	}
	if (form.rate.on) shaping.rate_limit = { max: form.rate.max ?? 0, window_s: toSeconds(form.rate.window) };
	if (!form.notifyResolved) shaping.notify_resolved = false;
	return shaping;
}

function listed(words: string[]): string {
	return words.length <= 1 ? words.join('') : `${words.slice(0, -1).join(', ')} and ${words[words.length - 1]}`;
}

/** A policy in plain words on one line, for the route list. Empty when the route sends everything at once. */
export function summarizeShaping(shaping: Shaping | undefined): string {
	if (!shaping) return '';
	const parts: string[] = [];
	const { dedup, group, quiet_hours: quiet, rate_limit: rate } = shaping;
	if (dedup) parts.push(`counts duplicates for ${spanWords(dedup.window_s)}`);
	if (group) {
		const by = group.by ?? [];
		parts.push(`groups ${by.length > 0 ? `by ${listed(by)}` : 'everything'} for ${spanWords(group.wait_s)}`);
		if (group.repeat_s) parts.push(`sends again every ${spanWords(group.repeat_s)}`);
	}
	if (quiet) {
		const days = DAYS.filter((day) => (quiet.days ?? []).includes(day.key)).map((day) => day.label);
		const on = days.length > 0 ? ` on ${listed(days)}` : '';
		const bypass = quiet.bypass_severity;
		const except = !bypass ? '' : bypass === 'critical' ? ' except critical' : ` except ${bypass} or higher`;
		parts.push(`quiet ${quiet.start}–${quiet.end} ${quiet.tz}${on}${except}`);
	}
	if (rate) parts.push(`at most ${rate.max} per ${spanWords(rate.window_s)}`);
	if (shaping.notify_resolved === false) parts.push('does not announce resolutions');
	return parts.join(', ');
}

const STEP_WORDS: Record<string, string> = {
	silence: 'Silence',
	resolve: 'Resolved event',
	dedup: 'Duplicates',
	group: 'Grouping',
	quiet_hours: 'Quiet hours',
	rate_limit: 'Rate limit',
	send: 'Send'
};

/** The name of a shaping step in plain words. */
export function stepWords(step: string): string {
	return STEP_WORDS[step] ?? step.replaceAll('_', ' ');
}

/** Whether what shaping would do sends a message now, makes one wait, or sends nothing new. */
export function outcomeTone(outcome: string): 'sends' | 'waits' | 'quiet' {
	if (outcome === 'sent' || outcome === 'resolves an open notification') return 'sends';
	return outcome === 'opens a group' || outcome === 'held' ? 'waits' : 'quiet';
}
