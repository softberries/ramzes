<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import type { EventPage, EventRow } from '#lib/api/types.ts';
	import FilterBar from '#lib/components/FilterBar.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import Pagination from '#lib/components/Pagination.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import SortHeader from '#lib/components/SortHeader.svelte';
	import StateTag from '#lib/components/StateTag.svelte';
	import {
		FEED_PAGE_SIZES,
		filtersFromQuery,
		filtersToQuery,
		isFiltered,
		isLiveView,
		moveSelection,
		viewFromQuery,
		viewToApi,
		viewToQuery,
		type FeedFilters,
		type FeedSort,
		type FeedView
	} from '#lib/feed.ts';
	import { clock, outcomeOf } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { pageCount } from '#lib/table.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** Several changes arriving together cause one reload. */
	const REFRESH_DELAY_MS = 200;
	/** How long a newly arrived row stays highlighted. */
	const ARRIVAL_MS = 1600;

	const query = $derived(filtersToQuery(filtersFromQuery(page.url.searchParams)));
	const filters = $derived(filtersFromQuery(query));
	const viewQuery = $derived(viewToQuery(viewFromQuery(page.url.searchParams)));
	const view = $derived(viewFromQuery(viewQuery));
	/** Everything the list depends on; a request answered after it changed is out of date. */
	const asked = $derived(`${query}|${viewQuery}`);

	let rows = $state<EventRow[]>([]);
	let total = $state(0);
	let loaded = $state(false);
	let paused = $state(false);
	/** Events that arrived while paused. */
	let held = $state(0);
	let selected = $state<string | null>(null);
	let arrived = $state<string[]>([]);

	function fetchPage(): Promise<EventPage> {
		const prefix = query ? `${query}&` : '';
		return api<EventPage>('GET', `/api/events?${prefix}${viewToApi(view)}`);
	}

	// Load from scratch whenever the filters, the order or the page change.
	$effect(() => {
		const request = asked;
		loaded = false;
		held = 0;
		fetchPage()
			.then((answer) => {
				// Something changed while this was loading; a newer request is on its way.
				if (request !== asked) return;
				rows = answer.events;
				total = answer.total;
				loaded = true;
				// The list shrank under a page that no longer exists: go to the last one that does.
				const last = pageCount(answer.total, view.size);
				if (view.page > last) setView({ ...view, page: last });
			})
			.catch((error) => toasts.problem(error));
	});

	/** Brings in whatever changed since the list was loaded. */
	async function refresh() {
		const request = asked;
		const answer = await fetchPage().catch(() => null);
		if (!answer || request !== asked) return;
		if (paused) {
			held = Math.max(answer.total - total, 0);
			return;
		}
		const known = new Set(rows.map((row) => row.id));
		const fresh = isLiveView(view) ? answer.events.filter((row) => !known.has(row.id)).map((row) => row.id) : [];
		rows = answer.events;
		total = answer.total;
		held = 0;
		if (fresh.length > 0) {
			arrived = fresh;
			setTimeout(() => (arrived = arrived.filter((id) => !fresh.includes(id))), ARRIVAL_MS);
		}
	}

	$effect(() => {
		lookups.load().catch((error) => toasts.problem(error));
		let timer: ReturnType<typeof setTimeout> | undefined;
		const soon = () => {
			clearTimeout(timer);
			timer = setTimeout(refresh, REFRESH_DELAY_MS);
		};
		const offs = [live.on('event', soon), live.on('delivery', soon), live.on('resync', soon)];
		return () => {
			clearTimeout(timer);
			offs.forEach((off) => off());
		};
	});

	function go(nextFilters: FeedFilters, nextView: FeedView) {
		const search = [filtersToQuery(nextFilters), viewToQuery(nextView)].filter(Boolean).join('&');
		goto(resolve(search ? `/?${search}` : '/'), { replace: true, reset: false });
	}

	/** New filters start again from the first page. */
	function setFilters(next: FeedFilters) {
		go(next, { ...view, page: 1 });
	}

	function setView(next: FeedView) {
		go(filters, next);
	}

	/** A click on a heading: newest, most severe first; text from A. A second click turns it round. */
	function onsort(column: string) {
		const sort = column as FeedSort;
		const descendingFirst = sort === 'received' || sort === 'severity';
		setView({ ...view, page: 1, sort, descending: view.sort === sort ? !view.descending : descendingFirst });
	}

	function resume() {
		paused = false;
		refresh();
	}

	function onKeydown(event: KeyboardEvent) {
		const target = event.target as HTMLElement;
		if (['INPUT', 'SELECT', 'TEXTAREA'].includes(target.tagName) || event.metaKey || event.ctrlKey) return;
		if (event.key === 'j') selected = moveSelection(rows, selected, 1);
		else if (event.key === 'k') selected = moveSelection(rows, selected, -1);
		else if (event.key === 'Enter' && selected) goto(resolve('/events/[id]', { id: selected }));
		else return;
		event.preventDefault();
		document.getElementById(`event-${selected}`)?.scrollIntoView({ block: 'nearest' });
	}

	const sort = $derived({ key: view.sort, descending: view.descending });
</script>

<svelte:window onkeydown={onKeydown} />

<PageHeader title="Feed">
	Every event ramzes received, newest first, and what became of it.
	{#snippet actions()}
		{#if held > 0}
			<button type="button" class="btn btn-primary" onclick={resume}>
				Show {held} new {held === 1 ? 'event' : 'events'}
			</button>
		{/if}
		<button type="button" class="btn" aria-pressed={paused} onclick={() => (paused ? resume() : (paused = true))}>
			<Icon name={paused ? 'play' : 'pause'} />
			{paused ? 'Resume' : 'Pause'}
		</button>
	{/snippet}
</PageHeader>

<div class="card overflow-hidden">
	<div class="card-head">
		<div class="min-w-0 flex-1"><FilterBar {filters} sources={lookups.sources} onchange={setFilters} /></div>
	</div>

	<table class="table feed" aria-label="Events" aria-busy={!loaded}>
		<thead>
			<tr>
				<SortHeader column="received" label="Received" {sort} {onsort} />
				<SortHeader column="severity" label="Severity" {sort} {onsort} />
				<SortHeader column="title" label="Event" {sort} {onsort} />
				<SortHeader column="topic" label="Topic" {sort} {onsort} />
				<th scope="col">Source</th>
				<th scope="col">Sent to</th>
				<th scope="col" class="num">Outcome</th>
			</tr>
		</thead>
		<tbody>
			{#each rows as row (row.id)}
				<tr
					id="event-{row.id}"
					class="{row.id === selected ? 'selected' : ''} {arrived.includes(row.id) ? 'arrived' : ''}"
					onclick={(event) => {
						// A click anywhere on the row opens the event; the title is the link a keyboard reaches.
						if (!(event.target as HTMLElement).closest('a')) goto(resolve('/events/[id]', { id: row.id }));
					}}
				>
					<td><time class="text-muted whitespace-nowrap">{clock(row.received_at)}</time></td>
					<td><SeverityMark severity={row.severity} label /></td>
					<td>
						<a
							href={resolve('/events/[id]', { id: row.id })}
							class="title flex items-center gap-2 no-underline {row.status === 'resolved' ? 'text-muted' : 'text-fg'}"
						>
							<!-- The severity column is dropped on a phone; its mark moves in front of the title. -->
							<span class="md:hidden"><SeverityMark severity={row.severity} /></span>
							<span class="truncate">{row.title}</span>
						</a>
					</td>
					<td><span class="mono text-muted block truncate" title={row.topic}>{row.topic}</span></td>
					<td><span class="text-muted block truncate">{lookups.sourceNames[row.source_id] ?? ''}</span></td>
					<td>
						<span class="text-muted block truncate" title={row.targets.join(', ')}>{row.targets.join(', ')}</span>
					</td>
					<td class="num"><StateTag state={outcomeOf(row)} /></td>
				</tr>
			{:else}
				{#if loaded}
					<tr class="none">
						<td colspan="7" class="hint py-12 text-center">
							{#if isFiltered(filters)}
								No event matches these filters.
								<button type="button" class="text-accent underline" onclick={() => setFilters({})}>Clear filters</button>
							{:else}
								No events yet. Send one from a service: create a source under
								<a href={resolve('/sources')}>Sources</a> and use its example command.
							{/if}
						</td>
					</tr>
				{/if}
			{/each}
		</tbody>
	</table>

	{#if total > 0}
		<Pagination
			page={view.page}
			size={view.size}
			{total}
			sizes={FEED_PAGE_SIZES}
			noun="events"
			onpage={(next) => setView({ ...view, page: next })}
			onsize={(size) => setView({ ...view, page: 1, size })}
		/>
	{/if}
</div>

<p class="hint mt-4 hidden md:block">
	<kbd class="tag">j</kbd> <kbd class="tag">k</kbd> to move, <kbd class="tag">Enter</kbd> to open.
</p>

<style>
	/*
	 * Columns keep their widths whatever the rows hold; long text is cut, not wrapped. The sortable
	 * headings are drawn by a component of their own, so they are reached with `:global`.
	 */
	.feed {
		table-layout: fixed;
	}

	.feed :global(th:nth-child(1)) {
		width: 9.5rem;
	}

	.feed :global(th:nth-child(2)) {
		width: 7rem;
	}

	.feed :global(th:nth-child(4)) {
		width: 16%;
	}

	.feed :global(th:nth-child(5)) {
		width: 11%;
	}

	.feed :global(th:nth-child(6)) {
		width: 16%;
	}

	.feed :global(th:nth-child(7)) {
		width: 8.5rem;
	}

	.feed tbody tr:not(.none) {
		cursor: pointer;
	}

	.feed tr.selected {
		background: var(--raised);
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.title:hover {
		text-decoration: underline;
	}

	/* Narrower screens drop the columns that say least: where it went, where it came from, its topic. */
	@media (max-width: 80rem) {
		.feed :global(:is(th, td):nth-child(6)) {
			display: none;
		}
	}

	@media (max-width: 64rem) {
		.feed :global(:is(th, td):nth-child(5)) {
			display: none;
		}
	}

	@media (max-width: 48rem) {
		.feed :global(:is(th, td):nth-child(4)),
		.feed :global(:is(th, td):nth-child(2)) {
			display: none;
		}

		.feed :global(th:nth-child(1)) {
			width: 5.5rem;
		}

		.feed :global(th:nth-child(7)) {
			width: 7rem;
		}

		.feed :global(:is(th, td)) {
			padding-right: 0.6rem;
			padding-left: 0.6rem;
		}
	}

	/* The one piece of unprompted motion: a new row arrives lit and settles. */
	.arrived {
		animation: arrive 1.6s ease-out;
	}

	@keyframes arrive {
		from {
			background: color-mix(in srgb, var(--accent) 28%, transparent);
		}
		to {
			background: transparent;
		}
	}
</style>
