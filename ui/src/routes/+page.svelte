<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import type { EventPage, EventRow } from '#lib/api/types.ts';
	import FilterBar from '#lib/components/FilterBar.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import StateTag from '#lib/components/StateTag.svelte';
	import {
		filtersFromQuery,
		filtersToQuery,
		isFiltered,
		mergeNewest,
		moveSelection,
		type FeedFilters
	} from '#lib/feed.ts';
	import { clock, outcomeOf } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const PAGE_SIZE = 50;
	/** Several changes arriving together cause one reload. */
	const REFRESH_DELAY_MS = 200;
	/** How long a newly arrived row stays highlighted. */
	const ARRIVAL_MS = 1600;

	const query = $derived(filtersToQuery(filtersFromQuery(page.url.searchParams)));
	const filters = $derived(filtersFromQuery(query));

	let rows = $state<EventRow[]>([]);
	let nextBefore = $state<string | null>(null);
	let loaded = $state(false);
	let paused = $state(false);
	/** Events that arrived while paused. */
	let held = $state(0);
	let selected = $state<string | null>(null);
	let arrived = $state<string[]>([]);

	async function firstPage(): Promise<EventPage> {
		const prefix = query ? `${query}&` : '';
		return api<EventPage>('GET', `/api/events?${prefix}limit=${PAGE_SIZE}`);
	}

	// Load from scratch whenever the filters change.
	$effect(() => {
		const asked = query;
		loaded = false;
		held = 0;
		firstPage()
			.then((first) => {
				// The filters changed while this was loading; a newer request is on its way.
				if (asked !== query) return;
				rows = first.events;
				nextBefore = first.next_before ?? null;
				loaded = true;
			})
			.catch((error) => toasts.problem(error));
	});

	/** Brings in whatever changed since the list was loaded. */
	async function refresh() {
		const asked = query;
		const first = await firstPage().catch(() => null);
		if (!first || asked !== query) return;
		const merged = mergeNewest(rows, first.events, PAGE_SIZE);
		if (paused) {
			held = merged.added;
			return;
		}
		const known = new Set(rows.map((row) => row.id));
		const fresh = first.events.filter((row) => !known.has(row.id)).map((row) => row.id);
		rows = merged.rows;
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

	async function loadOlder() {
		if (!nextBefore) return;
		const prefix = query ? `${query}&` : '';
		try {
			const older = await api<EventPage>('GET', `/api/events?${prefix}limit=${PAGE_SIZE}&before=${nextBefore}`);
			rows = [...rows, ...older.events];
			nextBefore = older.next_before ?? null;
		} catch (error) {
			toasts.problem(error);
		}
	}

	function setFilters(next: FeedFilters) {
		const search = filtersToQuery(next);
		// The path is resolved; only the query string is appended to it.
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(`${resolve('/')}${search ? `?${search}` : ''}`, { replace: true, reset: false });
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
</script>

<svelte:window onkeydown={onKeydown} />

<div class="mb-4 flex flex-wrap items-center gap-3">
	<h1 class="mr-auto">Feed</h1>
	{#if held > 0}
		<button type="button" class="btn btn-primary" onclick={resume}>
			Show {held} new {held === 1 ? 'event' : 'events'}
		</button>
	{/if}
	<button type="button" class="btn" aria-pressed={paused} onclick={() => (paused ? resume() : (paused = true))}>
		{paused ? 'Resume' : 'Pause'}
	</button>
</div>

<FilterBar {filters} sources={lookups.sources} onchange={setFilters} />

<ul class="border-line mt-4 border-t" aria-label="Events" aria-busy={!loaded}>
	{#each rows as row (row.id)}
		<li>
		<a
			id="event-{row.id}"
			href={resolve('/events/[id]', { id: row.id })}
			class="feed-row border-line text-fg hover:bg-surface grid items-baseline gap-x-3 border-b px-2 py-1.5 no-underline
				{row.id === selected ? 'bg-raised' : ''} {arrived.includes(row.id) ? 'arrived' : ''}"
		>
			<time class="text-muted whitespace-nowrap">{clock(row.received_at)}</time>
			<SeverityMark severity={row.severity} />
			<span class="min-w-0 truncate">
				<span class={row.status === 'resolved' ? 'text-muted' : ''}>{row.title}</span>
				<span class="text-faint ml-2">{row.topic}</span>
			</span>
			<span class="text-muted hidden truncate lg:block">{lookups.sourceNames[row.source_id] ?? ''}</span>
			<span class="text-right"><StateTag state={outcomeOf(row)} /></span>
		</a>
		</li>
	{:else}
		{#if loaded}
			<li class="hint px-2 py-10">
				{#if isFiltered(filters)}
					No event matches these filters.
					<button type="button" class="text-accent underline" onclick={() => setFilters({})}>Clear filters</button>
				{:else}
					No events yet. Send one from a service: create a source under
					<a href={resolve('/sources')}>Sources</a> and use its example command.
				{/if}
			</li>
		{/if}
	{/each}
</ul>

{#if nextBefore}
	<div class="mt-4">
		<button type="button" class="btn" onclick={loadOlder}>Load older events</button>
	</div>
{/if}

<p class="hint mt-6 hidden md:block">
	<kbd class="tag">j</kbd> <kbd class="tag">k</kbd> to move, <kbd class="tag">Enter</kbd> to open.
</p>

<style>
	.feed-row {
		grid-template-columns: 5.5rem 0.6rem minmax(0, 1fr) 6.5rem;
	}

	@media (min-width: 64rem) {
		.feed-row {
			grid-template-columns: 5.5rem 0.6rem minmax(0, 1fr) 10rem 6.5rem;
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
