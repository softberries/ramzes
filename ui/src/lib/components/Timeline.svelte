<script lang="ts">
	import { resolve } from '$app/paths';
	import type { Journey, JourneyDelivery } from '#lib/api/types.ts';
	import { clock, duration, until } from '#lib/format.ts';
	import StateTag from './StateTag.svelte';

	interface Props {
		journey: Journey;
		/** Connection names by id. */
		connections: Record<string, string>;
		/** Source names by id. */
		sources: Record<string, string>;
		/** Current time in unix milliseconds. */
		now: number;
		/** Send one delivery again. */
		onreplay: (deliveryId: string) => void;
	}

	let { journey, connections, sources, now, onreplay }: Props = $props();

	const event = $derived(journey.event);

	function waiting(delivery: JourneyDelivery): boolean {
		return delivery.state === 'pending' || delivery.state === 'in_progress';
	}

	/** The colour of a step's node on the path. */
	function tone(delivery: JourneyDelivery): string {
		return `var(--st-${delivery.state})`;
	}
</script>

<!--
	The path an event took, top to bottom. Each node sits on one continuous line, so the eye
	can follow the event from receipt to the answer of the service it was sent to.
-->
<ol class="path">
	<li style="--node: var(--accent)">
		<div class="flex flex-wrap items-baseline gap-x-4">
			<span class="font-medium">Received from {sources[event.source_id] ?? 'a deleted source'}</span>
			<time class="text-muted">{clock(event.received_at, now)}</time>
		</div>
	</li>

	{#if !journey.routed}
		<li class="open" style="--node: var(--st-pending)">
			<span class="text-muted">Waiting to be routed</span>
		</li>
	{:else if journey.notifications.length === 0}
		<li class="open" style="--node: var(--st-unrouted)">
			<div class="font-medium" style="color: var(--st-unrouted)">No route matched</div>
			<p class="hint max-w-xl">
				Nothing was sent. Add or widen a route under
				<a href={resolve('/routes')}>Routes</a>, then run this event through the routes again.
			</p>
		</li>
	{/if}

	{#each journey.notifications as notification (notification.id)}
		<li style="--node: var(--accent)">
			<div class="flex flex-wrap items-baseline gap-x-4">
				<span class="font-medium">Matched route {notification.route_name}</span>
				<span class="text-muted">
					to {connections[notification.connection_id] ?? 'a deleted connection'}{notification.destination
						? ` (${notification.destination})`
						: ''}
				</span>
				{#if notification.replay}<span class="tag">replayed</span>{/if}
			</div>
		</li>

		{#each notification.deliveries as delivery (delivery.id)}
			<li class={waiting(delivery) ? 'open' : ''} style="--node: {tone(delivery)}">
				<div class="flex flex-wrap items-baseline gap-x-4">
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
			</li>
		{/each}
	{/each}
</ol>

<style>
	.path {
		--gutter: 1.75rem;
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 1.1rem;
		padding-left: var(--gutter);
	}

	/* The line every node sits on. */
	.path::before {
		content: '';
		position: absolute;
		top: 0.55rem;
		bottom: 0.55rem;
		left: 0.3rem;
		width: 2px;
		background: var(--line);
	}

	.path > li {
		position: relative;
	}

	.path > li::before {
		content: '';
		position: absolute;
		top: 0.35rem;
		left: calc(-1 * var(--gutter) + 0.05rem);
		width: 0.6rem;
		height: 0.6rem;
		background: var(--node);
	}

	/* A step that is still in progress is drawn hollow. */
	.path > li.open::before {
		background: var(--bg);
		box-shadow: inset 0 0 0 2px var(--node);
	}
</style>
