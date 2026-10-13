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

<div class="flex max-w-3xl flex-wrap items-center gap-x-3 gap-y-1">
	{#if label}<span class="font-medium break-words" data-testid="delivery-step">{label}</span>{/if}
	<StateTag state={delivery.state} />
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
		<button type="button" class="btn btn-sm ml-auto" onclick={() => onreplay(delivery.id)}>Send again</button>
	{/if}
</div>

{#if delivery.warning}<p class="text-gold mt-1 break-words">{delivery.warning}</p>{/if}
{#if delivery.attempts.length > 0}
	<!-- One grid for every attempt, so the columns line up; on a phone the answer drops to a line of its own. -->
	<div class="attempts mt-2 max-w-3xl">
		{#each delivery.attempts as attempt (attempt.id)}
			<time class="text-muted whitespace-nowrap">{clock(attempt.started_at, now)}</time>
			<span class="mono whitespace-nowrap">
				{attempt.status === null || attempt.status === undefined ? 'no answer' : `HTTP ${attempt.status}`}
			</span>
			<span class="mono text-muted whitespace-nowrap">{duration(attempt.duration_ms)}</span>
			<span class="detail {attempt.outcome === 'delivered' ? 'text-muted' : 'problem'} break-words" class:blank={!attempt.detail}>
				{attempt.detail ?? ''}
			</span>
		{/each}
	</div>
{:else if delivery.last_error}
	<p class="problem mt-1 break-words">{delivery.last_error}</p>
{/if}

<style>
	.attempts {
		display: grid;
		grid-template-columns: auto auto 1fr;
		gap: 0.15rem 1.25rem;
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius-control);
		background: color-mix(in srgb, var(--raised) 45%, transparent);
	}

	.detail {
		grid-column: 1 / -1;
	}

	.detail.blank {
		display: none;
	}

	@media (min-width: 48rem) {
		.attempts {
			grid-template-columns: auto auto auto 1fr;
		}

		.detail {
			grid-column: auto;
		}

		/* It still takes its cell, so the next attempt starts on a new row. */
		.detail.blank {
			display: block;
		}
	}
</style>
