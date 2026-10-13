import type { EscalationStep, NotificationRow, StepProgress, Target } from './api/types.ts';
import { countdown, timeOfDay } from './format.ts';
import { shortWho } from './notifications.ts';
import { cleanTargets } from './routes.ts';
import { listed, splitSeconds, toSeconds, type Span } from './shaping.ts';

/** One row of a target editor. Empty strings stand for "not chosen". */
export type TargetRow = Required<Target>;

/** One step of an escalation chain as its form holds it. */
export interface StepForm {
	/** How long after the previous step (or the first message) this one goes out. */
	after: Span;
	targets: TargetRow[];
}

export function emptyTarget(connectionId: string = ''): TargetRow {
	return { connection_id: connectionId, destination: '', template_id: '' };
}

/** A target as a form row. */
export function targetRow(target: Target): TargetRow {
	return {
		connection_id: target.connection_id,
		destination: target.destination ?? '',
		template_id: target.template_id ?? ''
	};
}

/** A step as it is added to the form: 15 minutes, one target to choose. */
export function emptyStep(): StepForm {
	return { after: { amount: 15, unit: 'min' }, targets: [emptyTarget()] };
}

/** The form showing a stored chain. A route without one has no steps. */
export function chainToForm(chain: EscalationStep[] | undefined): StepForm[] {
	return (chain ?? []).map((step) => ({ after: splitSeconds(step.after_s), targets: step.targets.map(targetRow) }));
}

/**
 * The chain to send to the API, in the order of the form. Targets are cleaned as a route's own are.
 * A step that is not filled in is kept, so the server says what is missing in it.
 */
export function formToChain(steps: StepForm[], takesDestination: (connectionId: string) => boolean): EscalationStep[] {
	return steps.map((step) => ({ after_s: toSeconds(step.after), targets: cleanTargets(step.targets, takesDestination) }));
}

/** The delay of each step of a form, in seconds. */
export function delaysOf(steps: StepForm[]): number[] {
	return steps.map((step) => toSeconds(step.after));
}

/** From the delay of each step, the seconds between the first message and that step. */
export function cumulativeSeconds(delays: number[]): number[] {
	let total = 0;
	return delays.map((delay) => (total += delay));
}

/** A sum of delays in words. The delays may be in different units, so it takes up to two: "45 min 3 s". */
function sumWords(seconds: number): string {
	return countdown(seconds * 1000);
}

/** When a step goes out, counted from the first message: "45 min after the first message". */
export function sinceFirstWords(seconds: number): string {
	return `${sumWords(seconds)} after the first message`;
}

function named(name: string | null | undefined, destination: string | null | undefined): string {
	return `${name ?? 'a deleted connection'}${destination ? ` (${destination})` : ''}`;
}

/** A chain on one line, for the route list. Times count from the first message. Empty without a chain. */
export function summarizeChain(chain: EscalationStep[] | undefined, connectionNames: Record<string, string>): string {
	const steps = chain ?? [];
	const since = cumulativeSeconds(steps.map((step) => step.after_s));
	return steps
		.map((step, index) => {
			const who = listed(step.targets.map((t) => named(connectionNames[t.connection_id], t.destination)));
			return `then ${who} after ${sumWords(since[index])}`;
		})
		.join(', ');
}

/** Who a step of a notification's chain tells. */
export function targetWords(targets: StepProgress['targets']): string {
	return listed(targets.map((t) => named(t.connection_name, t.destination)));
}

/** How far a notification is with one step of its chain. */
export function stepState(step: StepProgress): 'sent' | 'due' | 'not_reached' {
	if (step.sent_at) return 'sent';
	return step.due_at ? 'due' : 'not_reached';
}

/** The state of a step in words: "sent at 14:32", "due in 4 min 12 s", "not reached". */
export function stepWords(step: StepProgress, now: number): string {
	if (step.sent_at) return `sent at ${timeOfDay(step.sent_at, now)}`;
	if (!step.due_at) return 'not reached';
	return step.due_at > now ? `due in ${countdown(step.due_at - now)}` : 'due now';
}

/**
 * Why the rest of a chain will not go out: "stopped: acknowledged by kris@… at 14:35".
 * Empty while the chain is running, and when every step went out before anyone reacted.
 */
export function stoppedWords(notification: NotificationRow, steps: StepProgress[], now: number): string {
	if (steps.every((step) => step.sent_at)) return '';
	// An acknowledgement comes first: a later resolution finds the chain already stopped.
	if (notification.acked_at) {
		return `stopped: acknowledged by ${shortWho(notification.acked_by)} at ${timeOfDay(notification.acked_at, now)}`;
	}
	if (notification.state === 'acknowledged') return `stopped: acknowledged by ${shortWho(notification.acked_by)}`;
	if (notification.state === 'resolved') {
		return notification.resolved_at ? `stopped: resolved at ${timeOfDay(notification.resolved_at, now)}` : 'stopped: resolved';
	}
	return '';
}

/** What to call a delivery an escalation step sent. Empty for one to the route's own targets. */
export function stepLabel(
	delivery: { step?: number | null; connection_id: string; destination?: string | null },
	connectionNames: Record<string, string>
): string {
	if (delivery.step === null || delivery.step === undefined) return '';
	return `Step ${delivery.step + 1} to ${named(connectionNames[delivery.connection_id], delivery.destination)}`;
}

/** When the first message of a notification went out, or null when none has. */
export function firstSentAt(deliveries: { step?: number | null; created_at: number }[]): number | null {
	const own = deliveries.filter((d) => d.step === null || d.step === undefined).map((d) => d.created_at);
	return own.length > 0 ? Math.min(...own) : null;
}
