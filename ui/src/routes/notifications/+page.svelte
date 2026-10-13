<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import type { NotificationPage, NotificationRow, Overview, Settings } from '#lib/api/types.ts';
	import NotificationActions from '#lib/components/NotificationActions.svelte';
	import NotificationState from '#lib/components/NotificationState.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import { mergeNewest } from '#lib/feed.ts';
	import { dismiss, isDismissed, lacksPublicUrl } from '#lib/hints.ts';
	import { TABS, stateQuery, tabOf } from '#lib/notifications.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const PAGE_SIZE = 50;
	/** Several changes arriving together cause one reload. */
	const REFRESH_DELAY_MS = 200;

	const tab = $derived(tabOf(page.url.searchParams.get('view')));

	let rows = $state<NotificationRow[]>([]);
	let nextBefore = $state<string | null>(null);
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

	function firstPage(): Promise<NotificationPage> {
		return api<NotificationPage>('GET', `/api/notifications?${stateQuery(tab)}limit=${PAGE_SIZE}`);
	}

	async function loadCounts() {
		const overview = await api<Overview>('GET', '/api/overview?hours=1');
		counts = { waiting: overview.waiting, sent: overview.unacknowledged };
	}

	// Load from scratch whenever the view changes.
	$effect(() => {
		const asked = tab;
		loaded = false;
		firstPage()
			.then((first) => {
				// The view changed while this was loading; a newer request is on its way.
				if (asked !== tab) return;
				rows = first.notifications;
				nextBefore = first.next_before ?? null;
				loaded = true;
			})
			.catch((error) => toasts.problem(error));
	});

	/** Brings in whatever changed since the list was loaded. */
	async function refresh() {
		const asked = tab;
		loadCounts().catch(() => {});
		const first = await firstPage().catch(() => null);
		if (!first || asked !== tab) return;
		rows = mergeNewest(rows, first.notifications, PAGE_SIZE).rows;
		if (first.notifications.length < PAGE_SIZE) nextBefore = null;
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

	async function loadOlder() {
		if (!nextBefore) return;
		try {
			const older = await api<NotificationPage>(
				'GET',
				`/api/notifications?${stateQuery(tab)}limit=${PAGE_SIZE}&before=${nextBefore}`
			);
			rows = [...rows, ...older.notifications];
			nextBefore = older.next_before ?? null;
		} catch (error) {
			toasts.problem(error);
		}
	}

	function countOf(key: string): number | null {
		if (!counts) return null;
		return key === 'waiting' ? counts.waiting : key === 'sent' ? counts.sent : null;
	}
</script>

<h1 class="mb-2">Notifications</h1>
<p class="hint mb-4 max-w-2xl">
	A notification is what one route tells one connection about: a single event, or several counted or collected
	into one message. Acknowledge one to say someone is on it; reminders stop, and one still waiting is not sent.
</p>

{#if noAckLink}
	<div class="border-line mb-4 flex max-w-2xl items-start gap-3 border-l-2 pl-3" data-testid="no-ack-link">
		<p class="hint">
			Messages will have no acknowledgement link until the address of this ramzes instance is set under
			<a href={resolve('/settings')}>Settings</a>.
		</p>
		<button type="button" class="btn h-7 px-2" onclick={dismissAckHint}>Dismiss</button>
	</div>
{/if}

<nav class="mb-4 flex flex-wrap gap-1" aria-label="Notifications to show">
	{#each TABS as item (item.key)}
		{@const count = countOf(item.key)}
		<a
			href={resolve(item.key === TABS[0].key ? '/notifications' : `/notifications?view=${item.key}`)}
			aria-current={item.key === tab.key ? 'page' : undefined}
			data-sveltekit-replacestate
			class="btn no-underline {item.key === tab.key ? 'border-gold bg-raised text-fg' : 'text-muted'}"
		>
			{item.label}
			{#if count !== null && count > 0}<span class="text-gold" data-testid="count-{item.key}">{count}</span>{/if}
		</a>
	{/each}
</nav>

<ul class="border-line border-t" aria-label="Notifications" aria-busy={!loaded}>
	{#each rows as row (row.id)}
		<li class="note-row border-line grid items-baseline gap-x-3 gap-y-1.5 border-b px-2 py-2.5">
			<SeverityMark severity={row.severity} />
			<div class="min-w-0">
				<div class="flex flex-wrap items-baseline gap-x-3">
					<a href={resolve('/notifications/[id]', { id: row.id })} class="text-fg font-medium break-words">{row.title}</a>
					{#if row.event_count > 1}<span class="tag" data-testid="event-count">{row.event_count} events</span>{/if}
				</div>
				<div class="text-muted break-words">
					{row.route_name} to
					<span class={row.connection_name ? '' : 'problem'}>{row.connection_name ?? 'a deleted connection'}</span
					>{row.destination ? ` (${row.destination})` : ''}
				</div>
			</div>
			<div class="state min-w-0"><NotificationState notification={row} {now} /></div>
			<div class="actions xl:justify-self-end"><NotificationActions notification={row} onchange={refresh} /></div>
		</li>
	{:else}
		{#if loaded}
			<li class="hint max-w-2xl px-2 py-10">
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
			</li>
		{/if}
	{/each}
</ul>

{#if nextBefore}
	<div class="mt-4">
		<button type="button" class="btn" onclick={loadOlder}>Load older notifications</button>
	</div>
{/if}

<style>
	/* On a phone: the title beside its mark, then the state, then the buttons, each on its own line. */
	.note-row {
		grid-template-columns: 0.6rem minmax(0, 1fr);
	}

	.note-row .state {
		grid-column: 2;
	}

	/* The buttons start at the edge, so all three fit on one line. */
	.note-row .actions {
		grid-column: 1 / -1;
	}

	@media (min-width: 80rem) {
		.note-row {
			/* Fixed widths, so the states and the buttons line up from row to row. */
			grid-template-columns: 0.6rem minmax(0, 1fr) 23.5rem 25.5rem;
		}

		.note-row .state,
		.note-row .actions {
			grid-column: auto;
		}
	}
</style>
