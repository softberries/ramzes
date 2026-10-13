<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api/client.ts';
	import type { NotificationView } from '#lib/api/types.ts';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import DeliveryBlock from '#lib/components/DeliveryBlock.svelte';
	import EscalationProgress from '#lib/components/EscalationProgress.svelte';
	import NotificationActions from '#lib/components/NotificationActions.svelte';
	import NotificationState from '#lib/components/NotificationState.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import { stepLabel } from '#lib/escalation.ts';
	import { clock } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	type NotificationEvent = NotificationView['events'][number];

	/** From this many events on, the list of events gets a search box. */
	const SEARCH_FROM = 10;

	const id = $derived(page.params.id ?? '');

	let notification = $state<NotificationView | null>(null);
	let missing = $state(false);
	let now = $state(Date.now());

	async function load() {
		try {
			notification = await api<NotificationView>('GET', `/api/notifications/${id}`);
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
		// Deliveries of escalation steps go to other connections; their names are needed to say where.
		lookups.load().catch((error) => toasts.problem(error));
		const offs = [
			live.on('notification', (m) => m.notification_id === id && load()),
			// A delivery message names its events, not its notification, so any of them is a reason to look.
			live.on('delivery', load),
			live.on('resync', load)
		];
		// Keeps "sends in…", "due in…" and "next attempt in…" current.
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

	const columns: Column<NotificationEvent>[] = [
		{ key: 'received', label: 'Received', sort: (event) => event.received_at, descendingFirst: true, from: 'md' },
		{ key: 'event', label: 'Event', sort: (event) => event.title },
		{ key: 'topic', label: 'Topic', sort: (event) => event.topic, from: 'md' },
		{ key: 'role', label: 'Role', sort: (event) => event.role, end: true }
	];
</script>

<p class="text-muted mb-3">
	<a href={resolve('/notifications')}>Notifications</a> <span class="text-faint">/</span> Notification
</p>

{#if missing}
	<h1>This notification does not exist</h1>
	<p class="hint mt-2">
		It may have been removed after its retention period. See the ones that are left under
		<a href={resolve('/notifications?view=all')}>Notifications</a>.
	</p>
{:else if notification}
	<header class="mb-5 flex flex-wrap items-start gap-x-6 gap-y-3">
		<div class="mr-auto min-w-0">
			<h1 class="break-words">{notification.title}</h1>
			<div class="text-muted mt-1 flex flex-wrap gap-x-5 gap-y-1">
				<SeverityMark severity={notification.severity} label />
				<span class="break-words">
					{notification.route_name} to
					<span class={notification.connection_name ? '' : 'problem'}
						>{notification.connection_name ?? 'a deleted connection'}</span
					>{notification.destination ? ` (${notification.destination})` : ''}
				</span>
			</div>
		</div>
		<NotificationActions {notification} onchange={load} />
	</header>

	<dl class="facts card mb-4">
		<div class="wide">
			<dt>State</dt>
			<dd><NotificationState {notification} {now} /></dd>
		</div>
		<div>
			<dt>Opened</dt>
			<dd><time>{clock(notification.created_at, now)}</time></dd>
		</div>
		<div>
			<dt>Events</dt>
			<dd class="tabular-nums">{notification.event_count}</dd>
		</div>
		<div>
			<dt>Messages sent</dt>
			<dd class="tabular-nums">{notification.sent_count}</dd>
		</div>
		{#if notification.acked_by}
			<div class="wide">
				<dt>Acknowledged by</dt>
				<dd class="break-all">{notification.acked_by}</dd>
			</div>
		{/if}
	</dl>

	{#snippet cell(event: NotificationEvent, column: Column<NotificationEvent>)}
		{#if column.key === 'received'}
			<time class="text-muted whitespace-nowrap">{clock(event.received_at, now)}</time>
		{:else if column.key === 'event'}
			<div class="flex items-baseline gap-2.5">
				<SeverityMark severity={event.severity} />
				<span class="min-w-0 break-words">
					<a href={resolve('/events/[id]', { id: event.id })}>{event.title}</a>
					<!-- The topic and the time are columns of their own from a tablet up. -->
					<span class="mono text-muted block break-all md:hidden">{event.topic}</span>
					<time class="text-faint block md:hidden">{clock(event.received_at, now)}</time>
				</span>
			</div>
		{:else if column.key === 'topic'}
			<span class="mono text-muted break-all">{event.topic}</span>
		{:else}
			<span class="tag" data-testid="event-role">{event.role}</span>
		{/if}
	{/snippet}

	{@const chained = notification.escalation.length > 0}
	<!-- On a wide screen the chain stands beside the other two; on a narrow one it follows what was sent. -->
	<div class="grid items-start gap-4 {chained ? 'xl:grid-cols-3' : ''}">
		<section class="card min-w-0 {chained ? 'xl:col-span-2' : ''}" aria-label="Deliveries">
			<div class="card-head"><h2>Deliveries</h2></div>
			{#if notification.deliveries.length === 0}
				<p class="card-body hint">
					{#if notification.state === 'grouping' || notification.state === 'held'}
						Nothing has been sent yet. A delivery appears here when the wait is over.
					{:else if notification.state === 'suppressed'}
						Nothing was sent, and nothing will be.
					{:else}
						Nothing was sent for this notification.
					{/if}
				</p>
			{:else}
				<ul class="divide-line divide-y">
					{#each notification.deliveries as delivery (delivery.id)}
						<li class="px-[1.1rem] py-3.5">
							<DeliveryBlock
								{delivery}
								{now}
								onreplay={replayDelivery}
								label={stepLabel(delivery, lookups.connectionNames)}
							/>
						</li>
					{/each}
				</ul>
			{/if}
		</section>

		{#if chained}
			<section class="card xl:row-span-2" aria-label="If nobody acknowledges">
				<div class="card-head"><h2>If nobody acknowledges</h2></div>
				<div class="card-body">
					<p class="hint mb-4">
						The escalation chain of {notification.route_name} as it is set up now. A step goes out only while the
						notification is sent and not acknowledged.
					</p>
					<EscalationProgress {notification} {now} />
				</div>
			</section>
		{/if}

		<section class="events min-w-0 {chained ? 'xl:col-span-2' : ''}" aria-label="Events">
			<!-- A notification that collected many events is searched; a short list needs no search box. -->
			<DataTable
				rows={notification.events}
				{columns}
				key={(event) => event.id}
				noun="events"
				singular="event"
				{cell}
				search={notification.events.length > SEARCH_FROM
					? (event) => `${event.title} ${event.topic} ${event.severity} ${event.role}`
					: undefined}
				searchPlaceholder="Search title or topic"
			>
				{#snippet toolbar()}<h2 class="-order-1">Events</h2>{/snippet}
				{#snippet empty()}Its events have been removed after their retention period.{/snippet}
			</DataTable>
		</section>

	</div>
{/if}

<style>
	/* What the notification is, in a row of figures: two to a line on a phone, all on one from a tablet up. */
	.facts {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 1rem 1.5rem;
		padding: 1rem 1.1rem;
	}

	.facts .wide {
		grid-column: 1 / -1;
	}

	.facts dt {
		margin-bottom: 0.15rem;
		color: var(--muted);
		font-size: 0.93rem;
	}

	@media (width < 48rem) {
		.events :global(:is(th, td)) {
			padding-right: 0.75rem;
			padding-left: 0.75rem;
		}
	}

	@media (min-width: 48rem) {
		.facts {
			display: flex;
			flex-wrap: wrap;
			gap: 1rem 3rem;
		}
	}
</style>
