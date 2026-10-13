<script lang="ts">
	import type { NotificationView } from '#lib/api/types.ts';
	import {
		cumulativeSeconds,
		firstSentAt,
		sinceFirstWords,
		stepState,
		stepWords,
		stoppedWords,
		targetWords
	} from '#lib/escalation.ts';
	import { timeOfDay } from '#lib/format.ts';

	interface Props {
		/** A notification of a route that has an escalation chain. */
		notification: NotificationView;
		/** Current time in unix milliseconds. Pass a ticking value to keep the countdown running. */
		now: number;
	}

	let { notification, now }: Props = $props();

	const steps = $derived(notification.escalation);
	const since = $derived(cumulativeSeconds(steps.map((step) => step.after_s)));
	const first = $derived(firstSentAt(notification.deliveries));
	const stopped = $derived(stoppedWords(notification, steps, now));
	/** The stop is shown where it happened: after the last step that went out. */
	const stoppedAfter = $derived(steps.filter((step) => step.sent_at).length);
	const waiting = $derived(notification.state === 'grouping' || notification.state === 'held');

	const TONES = { sent: '--accent', due: '--gold', not_reached: '--faint' } as const;
	const TEXT = { sent: 'text-fg', due: 'text-gold', not_reached: 'text-muted' } as const;
</script>

{#snippet stop()}
	<li style="--node: var({notification.acked_at ? '--st-delivered' : '--muted'})" data-testid="chain-stopped">
		<span class="font-medium break-words {notification.acked_at ? 'text-ok' : 'text-muted'}" title={notification.acked_by ?? undefined}
			>{stopped}</span
		>
	</li>
{/snippet}

<ol class="path" aria-label="Escalation chain">
	<li class={first ? '' : 'open'} style="--node: var({first ? '--accent' : waiting ? '--gold' : '--faint'})">
		<div class="flex flex-wrap items-baseline gap-x-4">
			<span class="font-medium">First message</span>
			<span class="text-muted break-words">
				to {notification.connection_name ?? 'a deleted connection'}{notification.destination
					? ` (${notification.destination})`
					: ''}
			</span>
		</div>
		<div class={first ? 'text-fg' : waiting ? 'text-gold' : 'text-muted'}>
			{first ? `sent at ${timeOfDay(first, now)}` : waiting ? 'not sent yet' : 'not sent'}
		</div>
	</li>
	{#each steps as step, index (index)}
		{@const state = stepState(step)}
		{#if stopped && index === stoppedAfter}{@render stop()}{/if}
		<li class={state === 'sent' ? '' : 'open'} style="--node: var({TONES[state]})" data-state={state}>
			<div class="flex flex-wrap items-baseline gap-x-4">
				<span class="font-medium">Step {index + 1}</span>
				<span class="text-muted break-words">to {targetWords(step.targets)}, {sinceFirstWords(since[index])}</span>
			</div>
			<div class={TEXT[state]} data-testid="step-state">{stepWords(step, now)}</div>
		</li>
	{/each}
</ol>
