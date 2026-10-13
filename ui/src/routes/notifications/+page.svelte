<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import type { NotificationPage, NotificationRow, Overview, Settings } from '#lib/api/types.ts';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import NotificationActions from '#lib/components/NotificationActions.svelte';
	import NotificationState from '#lib/components/NotificationState.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import Segments from '#lib/components/Segments.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import { clock } from '#lib/format.ts';
	import { dismiss, isDismissed, lacksPublicUrl } from '#lib/hints.ts';
	import { TABS, stateQuery, stateWords, tabOf } from '#lib/notifications.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** The most the API gives at once. The table searches, orders and pages these itself. */
	const LIMIT = 200;
	/** Several changes arriving together cause one reload. */
	const REFRESH_DELAY_MS = 200;

	const tab = $derived(tabOf(page.url.searchParams.get('view')));

	let rows = $state<NotificationRow[]>([]);
	/** Whether the view holds more than the newest `LIMIT`. */
	let more = $state(false);
	let loaded = $state(false);
	let now = $state(Date.now());
	/** How many are waiting and how many were sent and not acknowledged, shown on those two views. */
	let counts = $state<{ waiting: number; sent: number } | null>(null);
	/** Messages carry an acknowledgement link only when the instance knows its own address. */
	let noAckLink = $state(false);

	$effect(() => {
		if (isDismissed(localStorage, 'no-public-url')) return;
		// The hint is an extra: when the settings cannot be read, the page works without it.
		api<Settings>('GET', '/api/settings')
			.then((settings) => (noAckLink = lacksPublicUrl(settings)))
			.catch(() => {});
	});

	function dismissAckHint() {
		dismiss(localStorage, 'no-public-url');
		noAckLink = false;
	}

	function newest(): Promise<NotificationPage> {
		return api<NotificationPage>('GET', `/api/notifications?${stateQuery(tab)}limit=${LIMIT}`);
	}

	async function loadCounts() {
		const overview = await api<Overview>('GET', '/api/overview?hours=1');
		counts = { waiting: overview.waiting, sent: overview.unacknowledged };
	}

	// Load from scratch whenever the view changes.
	$effect(() => {
		const asked = tab;
		loaded = false;
		newest()
			.then((answer) => {
				// The view changed while this was loading; a newer request is on its way.
				if (asked !== tab) return;
				rows = answer.notifications;
				more = Boolean(answer.next_before);
				loaded = true;
			})
			.catch((error) => toasts.problem(error));
	});

	/** Brings in whatever changed since the list was loaded. */
	async function refresh() {
		const asked = tab;
		loadCounts().catch(() => {});
		const answer = await newest().catch(() => null);
		if (!answer || asked !== tab) return;
		rows = answer.notifications;
		more = Boolean(answer.next_before);
	}

	$effect(() => {
		loadCounts().catch((error) => toasts.problem(error));
		let timer: ReturnType<typeof setTimeout> | undefined;
		const soon = () => {
			clearTimeout(timer);
			timer = setTimeout(refresh, REFRESH_DELAY_MS);
		};
		const offs = [live.on('notification', soon), live.on('resync', soon)];
		// Keeps "sends in…" current.
		const tick = setInterval(() => (now = Date.now()), 1000);
		return () => {
			clearTimeout(timer);
			clearInterval(tick);
			offs.forEach((off) => off());
		};
	});

	function countOf(key: string): number | null {
		if (!counts) return null;
		return key === 'waiting' ? counts.waiting : key === 'sent' ? counts.sent : null;
	}

	const connectionOf = (row: NotificationRow) =>
		`${row.connection_name ?? 'a deleted connection'}${row.destination ? ` (${row.destination})` : ''}`;

	const columns: Column<NotificationRow>[] = [
		{ key: 'title', label: 'Notification', sort: (row) => row.title },
		{ key: 'state', label: 'State', sort: (row) => stateWords(row, now).state, width: 'min(11.5rem, 36vw)' },
		{ key: 'route', label: 'Route', sort: (row) => row.route_name },
		{ key: 'connection', label: 'Connection', sort: connectionOf },
		{ key: 'events', label: 'Events', sort: (row) => row.event_count, descendingFirst: true, end: true },
		{ key: 'opened', label: 'Opened', sort: (row) => row.created_at, descendingFirst: true, from: 'md' },
		{ key: 'actions', label: 'Actions', from: 'md', width: '1%' }
	];

	function searchText(row: NotificationRow): string {
		const words = stateWords(row, now);
		return [row.title, row.route_name, connectionOf(row), row.severity, words.state, words.reason].join(' ');
	}
</script>

<PageHeader title="Notifications">
	A notification is what one route tells one connection about: a single event, or several counted or collected
	into one message. Acknowledge one to say someone is on it; reminders stop, and one still waiting is not sent.
</PageHeader>

{#if noAckLink}
	<div class="card mb-4 flex max-w-3xl flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3" data-testid="no-ack-link">
		<span class="text-gold"><Icon name="alert" /></span>
		<p class="text-muted min-w-0 flex-1 basis-64">
			Messages will have no acknowledgement link until the address of this ramzes instance is set under
			<a href={resolve('/settings')}>Settings</a>.
		</p>
		<button type="button" class="btn btn-sm" onclick={dismissAckHint}>Dismiss</button>
	</div>
{/if}

<nav class="mb-4" aria-label="Notifications to show">
	<Segments>
		{#each TABS as item (item.key)}
			{@const count = countOf(item.key)}
			<a
				href={resolve(item.key === TABS[0].key ? '/notifications' : `/notifications?view=${item.key}`)}
				aria-current={item.key === tab.key ? 'page' : undefined}
				data-sveltekit-replacestate
			>
				{item.label}
				{#if count !== null && count > 0}
					<span class="count" style="--tone: var(--gold)" data-testid="count-{item.key}">{count}</span>
				{/if}
			</a>
		{/each}
	</Segments>
</nav>

{#snippet cell(row: NotificationRow, column: Column<NotificationRow>)}
	{#if column.key === 'title'}
		<div class="flex items-baseline gap-2.5">
			<SeverityMark severity={row.severity} />
			<div class="min-w-0">
				<a href={resolve('/notifications/[id]', { id: row.id })} class="title text-fg font-medium break-words">
					{row.title}
				</a>
				<!-- Where it went and how many events it holds are columns of their own on a wide screen. -->
				<div class="text-muted break-words xl:hidden">
					{row.route_name} to
					<span class={row.connection_name ? '' : 'problem'}>{connectionOf(row)}</span
					>{row.event_count > 1 ? `, ${row.event_count} events` : ''}
				</div>
				<!-- So are the buttons, from a tablet up. -->
				<div class="mt-2 md:hidden"><NotificationActions notification={row} onchange={refresh} /></div>
			</div>
		</div>
	{:else if column.key === 'state'}
		<NotificationState notification={row} {now} />
	{:else if column.key === 'route'}
		<span class="text-muted break-words">{row.route_name}</span>
	{:else if column.key === 'connection'}
		<span class="break-words {row.connection_name ? 'text-muted' : 'problem'}">{connectionOf(row)}</span>
	{:else if column.key === 'events'}
		{#if row.event_count > 1}
			<span class="font-medium" data-testid="event-count">{row.event_count}</span>
		{:else}
			<span class="text-muted">1</span>
		{/if}
	{:else if column.key === 'opened'}
		<time class="text-muted whitespace-nowrap">{clock(row.created_at, now)}</time>
	{:else}
		<NotificationActions notification={row} onchange={refresh} />
	{/if}
{/snippet}

{#snippet empty()}
	<p class="mx-auto max-w-xl">
		{#if tab.key === 'waiting'}
			Nothing is waiting. A notification waits here while its route collects events into a group, or holds them
			for quiet hours or a rate limit. Set those up under <a href={resolve('/routes')}>Routes</a>, or see what
			was <a href={resolve('/notifications?view=sent')} data-sveltekit-replacestate>sent</a>.
		{:else if tab.key === 'sent'}
			Nothing is waiting to be acknowledged.
			<a href={resolve('/notifications?view=all')} data-sveltekit-replacestate>See every notification.</a>
		{:else if tab.key === 'acknowledged'}
			Nothing is acknowledged and still open. Acknowledge a notification under
			<a href={resolve('/notifications?view=sent')} data-sveltekit-replacestate>Sent</a> to say someone is on it.
		{:else if tab.key === 'resolved'}
			Nothing has been resolved yet. A notification is resolved by an event with the status resolved, or by
			hand with its Resolve button.
		{:else if tab.key === 'suppressed'}
			Nothing was held back. A notification is not sent while a silence covers its event; create one under
			<a href={resolve('/silences')}>Silences</a>.
		{:else}
			No notifications yet. One appears when an event matches a route: check
			<a href={resolve('/routes')}>Routes</a>, then send an event from a
			<a href={resolve('/sources')}>source</a>.
		{/if}
	</p>
{/snippet}

<div class="notes">
	<!-- Each view starts with its own search, order and page. -->
	{#key tab.key}
		<DataTable
			{rows}
			{columns}
			key={(row) => row.id}
			noun="notifications"
			singular="notification"
			{cell}
			search={searchText}
			searchPlaceholder="Search title, route, connection or state"
			initialSort={{ key: 'opened', descending: true }}
			{empty}
			{loaded}
		/>
	{/key}
</div>

{#if more}
	<p class="hint mt-3">
		These are the newest {LIMIT} notifications of this view. Older ones are not listed, and the search does not
		look in them.
	</p>
{/if}

<style>
	.title {
		text-decoration: none;
	}

	.title:hover {
		text-decoration: underline;
	}

	/*
	 * The table draws its own cells, so they are reached with `:global`. Route, connection and number
	 * of events move under the title below a width the table has no name for.
	 */
	@media (width < 80rem) {
		.notes :global(:is(th, td):nth-child(3)),
		.notes :global(:is(th, td):nth-child(4)),
		.notes :global(:is(th, td):nth-child(5)) {
			display: none;
		}
	}

	/* The buttons stay on one line, and their column is as wide as they are. */
	.notes :global(td:nth-child(7) > div) {
		flex-wrap: nowrap;
	}

	@media (width < 48rem) {
		.notes :global(:is(th, td)) {
			padding-right: 0.75rem;
			padding-left: 0.75rem;
		}

		.notes :global(td) {
			vertical-align: top;
		}
	}
</style>
