<script lang="ts" module>
	import type { JourneyDelivery } from '#lib/api/types.ts';

	/** Whether a delivery has not finished yet. */
	export function waiting(delivery: JourneyDelivery): boolean {
		return delivery.state === 'pending' || delivery.state === 'in_progress';
	}
</script>

<script lang="ts">
	import { clock, duration, until } from '#lib/format.ts';
	import StateTag from './StateTag.svelte';

	interface Props {
		/** One send of a notification, with the attempts made so far. */
		delivery: JourneyDelivery;
		/** Current time in unix milliseconds. */
		now: number;
		/** Send this delivery again. */
		onreplay: (deliveryId: string) => void;
		/** Which escalation step sent it and to whom. Empty for a delivery to the route's own targets. */
		label?: string;
	}

	let { delivery, now, onreplay, label = '' }: Props = $props();
</script>

<div class="flex flex-wrap items-baseline gap-x-4">
	{#if label}<span class="break-words" data-testid="delivery-step">{label}</span>{/if}
	<span class="font-medium"><StateTag state={delivery.state} /></span>
	{#if delivery.attempts.length > 0}
		<span class="text-muted">
			{delivery.attempts.length}
			{delivery.attempts.length === 1 ? 'attempt' : 'attempts'}
		</span>
	{/if}
	{#if delivery.state === 'pending' && delivery.attempts.length > 0}
		<span class="text-gold">Next attempt {until(delivery.next_attempt_at, now)}</span>
	{/if}
	{#if !waiting(delivery)}
		<button type="button" class="btn h-7 px-2" onclick={() => onreplay(delivery.id)}>Send again</button>
	{/if}
</div>

{#if delivery.warning}<p class="text-gold break-words">{delivery.warning}</p>{/if}
{#if delivery.attempts.length > 0}
	<table class="mt-1 w-full max-w-3xl">
		<tbody>
			{#each delivery.attempts as attempt (attempt.id)}
				<tr class="align-baseline">
					<td class="text-muted w-24 py-0.5 pr-4 whitespace-nowrap">{clock(attempt.started_at, now)}</td>
					<td class="w-20 py-0.5 pr-4 whitespace-nowrap">
						{attempt.status === null || attempt.status === undefined ? 'no answer' : `HTTP ${attempt.status}`}
					</td>
					<td class="text-muted w-20 py-0.5 pr-4 whitespace-nowrap">{duration(attempt.duration_ms)}</td>
					<td class="py-0.5 {attempt.outcome === 'delivered' ? 'text-muted' : 'problem'} break-words">
						{attempt.detail ?? ''}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
{:else if delivery.last_error}
	<p class="problem">{delivery.last_error}</p>
{/if}
