<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api/client.ts';
	import type { NotificationDetail } from '#lib/api/types.ts';
	import DeliveryBlock from '#lib/components/DeliveryBlock.svelte';
	import NotificationActions from '#lib/components/NotificationActions.svelte';
	import NotificationState from '#lib/components/NotificationState.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import { clock } from '#lib/format.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const id = $derived(page.params.id ?? '');

	let notification = $state<NotificationDetail | null>(null);
	let missing = $state(false);
	let now = $state(Date.now());

	async function load() {
		try {
			notification = await api<NotificationDetail>('GET', `/api/notifications/${id}`);
			missing = false;
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) missing = true;
			else toasts.problem(error);
		}
	}

	$effect(() => {
		void id;
		notification = null;
		load();
		const offs = [
			live.on('notification', (m) => m.notification_id === id && load()),
			// A delivery message names its events, not its notification, so any of them is a reason to look.
			live.on('delivery', load),
			live.on('resync', load)
		];
		// Keeps "sends in…" and "next attempt in…" current.
		const tick = setInterval(() => (now = Date.now()), 1000);
		return () => {
			offs.forEach((off) => off());
			clearInterval(tick);
		};
	});

	async function replayDelivery(deliveryId: string) {
		try {
			await api('POST', `/api/deliveries/${deliveryId}/replay`);
			toasts.ok('Queued to send again.');
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}
</script>

<p class="mb-3">
	<a href={resolve('/notifications')}>Notifications</a> <span class="text-faint">/</span> Notification
</p>

{#if missing}
	<h1>This notification does not exist</h1>
	<p class="hint mt-2">
		It may have been removed after its retention period. See the ones that are left under
		<a href={resolve('/notifications?view=all')}>Notifications</a>.
	</p>
{:else if notification}
	<header class="mb-6 flex flex-wrap items-start gap-x-6 gap-y-3">
		<div class="mr-auto min-w-0">
			<h1 class="break-words">{notification.title}</h1>
			<div class="text-muted mt-1 flex flex-wrap gap-x-5 gap-y-1">
				<SeverityMark severity={notification.severity} label />
				<span>
					{notification.route_name} to
					<span class={notification.connection_name ? '' : 'problem'}
						>{notification.connection_name ?? 'a deleted connection'}</span
					>{notification.destination ? ` (${notification.destination})` : ''}
				</span>
			</div>
		</div>
		<NotificationActions {notification} onchange={load} />
	</header>

	<dl class="text-muted mb-8 grid max-w-3xl grid-cols-[auto_1fr] gap-x-6 gap-y-1">
		<dt>State</dt>
		<dd class="text-fg"><NotificationState {notification} {now} /></dd>
		<dt>Opened</dt>
		<dd class="text-fg">{clock(notification.created_at, now)}</dd>
		<dt>Events</dt>
		<dd class="text-fg">{notification.event_count}</dd>
		<dt>Messages sent</dt>
		<dd class="text-fg">{notification.sent_count}</dd>
		{#if notification.acked_by}
			<dt>Acknowledged by</dt>
			<dd class="text-fg break-all">{notification.acked_by}</dd>
		{/if}
	</dl>

	<section class="mb-8" aria-label="Events">
		<h2 class="mb-2">Events</h2>
		<ul class="border-line border-t">
			{#each notification.events as event (event.id)}
				<li class="event-row border-line grid items-baseline gap-x-3 border-b px-2 py-1.5">
					<time class="text-muted whitespace-nowrap">{clock(event.received_at, now)}</time>
					<SeverityMark severity={event.severity} />
					<span class="min-w-0 break-words">
						<a href={resolve('/events/[id]', { id: event.id })}>{event.title}</a>
						<span class="text-faint ml-2">{event.topic}</span>
					</span>
					<span class="tag justify-self-end" data-testid="event-role">{event.role}</span>
				</li>
			{:else}
				<li class="hint border-line border-b px-2 py-2">Its events have been removed after their retention period.</li>
			{/each}
		</ul>
	</section>

	<section aria-label="Deliveries">
		<h2 class="mb-2">Deliveries</h2>
		{#if notification.deliveries.length === 0}
			<p class="hint max-w-2xl">
				{#if notification.state === 'grouping' || notification.state === 'held'}
					Nothing has been sent yet. A delivery appears here when the wait is over.
				{:else if notification.state === 'suppressed'}
					Nothing was sent, and nothing will be.
				{:else}
					Nothing was sent for this notification.
				{/if}
			</p>
		{:else}
			<ul class="border-line flex flex-col border-t">
				{#each notification.deliveries as delivery (delivery.id)}
					<li class="border-line border-b px-2 py-2.5">
						<DeliveryBlock {delivery} {now} onreplay={replayDelivery} />
					</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}

<style>
	.event-row {
		grid-template-columns: 5.5rem 0.6rem minmax(0, 1fr) auto;
	}
</style>
