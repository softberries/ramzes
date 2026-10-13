<script lang="ts">
	import { resolve } from '$app/paths';
	import type { Journey, JourneyDelivery, MessagePreview } from '#lib/api/types.ts';
	import { stepLabel } from '#lib/escalation.ts';
	import { clock } from '#lib/format.ts';
	import { journeyWords, roleWords } from '#lib/notifications.ts';
	import DeliveryBlock, { waiting } from './DeliveryBlock.svelte';
	import RequestBlock from './RequestBlock.svelte';

	interface Props {
		journey: Journey;
		/** Connection names by id. */
		connections: Record<string, string>;
		/** Source names by id. */
		sources: Record<string, string>;
		/** The message each notification carries, by notification id. */
		previews?: Record<string, MessagePreview>;
		/** Current time in unix milliseconds. */
		now: number;
		/** Send one delivery again. */
		onreplay: (deliveryId: string) => void;
	}

	let { journey, connections, sources, previews = {}, now, onreplay }: Props = $props();

	const event = $derived(journey.event);

	/** A phrase with its first letter in upper case. */
	function sentence(words: string): string {
		return words.charAt(0).toUpperCase() + words.slice(1);
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
		{@const preview = previews[notification.id]}
		{@const words = journeyWords(notification, now)}
		{@const role = sentence(roleWords(notification.role, notification.event_count))}
		<li class={words.text ? 'open' : ''} style="--node: var({words.text ? '--gold' : '--accent'})">
			<div class="flex flex-wrap items-baseline gap-x-4">
				<span class="font-medium">Matched route {notification.route_name}</span>
				<span class="text-muted">
					to {connections[notification.connection_id] ?? 'a deleted connection'}{notification.destination
						? ` (${notification.destination})`
						: ''}
				</span>
				{#if notification.replay}<span class="tag">replayed</span>{/if}
			</div>
			<div class="flex flex-wrap items-baseline gap-x-4">
				{#if words.text}
					<span>
						<span class="text-gold font-medium">{words.text}</span>
						{#if words.reason}<span class="text-muted break-words">{words.reason}</span>{/if}
					</span>
				{/if}
				{#if role}<span class="text-muted" data-testid="role">{role}.</span>{/if}
				<a href={resolve('/notifications/[id]', { id: notification.id })}>Open the notification</a>
			</div>
			{#if preview}
				<div class="panel mt-2 max-w-3xl p-3" data-testid="message">
					<div class="font-medium break-words">{preview.title}</div>
					{#if preview.body}<p class="text-muted mt-1 break-words whitespace-pre-wrap">{preview.body}</p>{/if}
					{#if preview.warning}<p class="text-gold mt-1 break-words">{preview.warning}</p>{/if}
					{#if preview.request}
						<details class="mt-2">
							<summary class="text-muted cursor-pointer">Request</summary>
							<div class="mt-1"><RequestBlock request={preview.request} /></div>
						</details>
					{:else if preview.request_error}
						<p class="hint mt-1">The request cannot be shown: {preview.request_error}</p>
					{/if}
				</div>
			{/if}
		</li>

		{#each notification.deliveries as delivery (delivery.id)}
			<li class={waiting(delivery) ? 'open' : ''} style="--node: {tone(delivery)}">
				<DeliveryBlock {delivery} {now} {onreplay} label={stepLabel(delivery, connections)} />
			</li>
		{/each}
	{/each}
</ol>
