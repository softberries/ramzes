<script lang="ts">
	import { resolve } from '$app/paths';
	import { api } from '#lib/api/client.ts';
	import type { FailureRow } from '#lib/api/types.ts';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import StateTag from '#lib/components/StateTag.svelte';
	import { errorOf, groupByError } from '#lib/failures.ts';
	import { clock } from '#lib/format.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let failures = $state<FailureRow[]>([]);
	let loaded = $state(false);
	let chosen = $state<string[]>([]);
	let busy = $state(false);
	/** The error whose failures are listed; empty for all of them. */
	let error = $state('');

	const groups = $derived(groupByError(failures));
	const listed = $derived(groups.find((group) => group.error === error)?.rows ?? failures);
	const allChosen = (rows: FailureRow[]) => rows.length > 0 && rows.every((row) => chosen.includes(row.id));

	async function load() {
		failures = await api<FailureRow[]>('GET', '/api/failures');
		// Forget selections of failures that are no longer listed.
		chosen = chosen.filter((id) => failures.some((f) => f.id === id));
		if (!failures.some((f) => errorOf(f) === error)) error = '';
		loaded = true;
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
		const offs = [live.on('delivery', () => load().catch(() => {})), live.on('resync', () => load().catch(() => {}))];
		return () => offs.forEach((off) => off());
	});

	function toggle(id: string) {
		chosen = chosen.includes(id) ? chosen.filter((c) => c !== id) : [...chosen, id];
	}

	/** Chooses every one of the failures given, or none of them when all already are. */
	function toggleAll(rows: FailureRow[]) {
		const ids = rows.map((row) => row.id);
		chosen = allChosen(rows) ? chosen.filter((id) => !ids.includes(id)) : [...new Set([...chosen, ...ids])];
	}

	async function replay(ids: string[]) {
		busy = true;
		try {
			const result = await api<{ replayed: number }>('POST', '/api/failures/replay', { delivery_ids: ids });
			toasts.ok(`Queued ${result.replayed} ${result.replayed === 1 ? 'delivery' : 'deliveries'} to send again.`);
			chosen = [];
			await load();
		} catch (error) {
			toasts.problem(error);
		} finally {
			busy = false;
		}
	}

	const connectionOf = (row: FailureRow) => row.connection_name ?? 'a deleted connection';

	const columns: Column<FailureRow>[] = [
		{ key: 'chosen', label: '', width: '2.75rem' },
		{ key: 'failed', label: 'Failed', sort: (row) => row.updated_at, descendingFirst: true, from: 'md' },
		{ key: 'event', label: 'Event', sort: (row) => row.event_title },
		{ key: 'route', label: 'Route', sort: (row) => row.route_name, from: 'lg' },
		{ key: 'connection', label: 'Connection', sort: connectionOf, from: 'lg' },
		{ key: 'error', label: 'Error', sort: errorOf, from: 'md' },
		{ key: 'attempts', label: 'Attempts', sort: (row) => row.attempt_count, descendingFirst: true, end: true, from: 'lg' },
		{ key: 'state', label: 'State', sort: (row) => row.state, end: true }
	];
</script>

<PageHeader title="Failures">
	Deliveries that were rejected or ran out of retries. Sending one again leaves the failed attempt in the
	event's history.
</PageHeader>

{#snippet cell(row: FailureRow, column: Column<FailureRow>)}
	{#if column.key === 'chosen'}
		<input
			type="checkbox"
			class="block"
			checked={chosen.includes(row.id)}
			onchange={() => toggle(row.id)}
			aria-label="Select {row.event_title}"
		/>
	{:else if column.key === 'failed'}
		<time class="text-muted whitespace-nowrap">{clock(row.updated_at)}</time>
	{:else if column.key === 'event'}
		<a href={resolve('/events/[id]', { id: row.event_id })} class="font-medium break-words">{row.event_title}</a>
		<!-- What a narrower screen has no column for moves under the title. -->
		<div class="text-muted break-words lg:hidden">{row.route_name} to {connectionOf(row)}</div>
		<div class="problem break-words md:hidden">{errorOf(row)}</div>
	{:else if column.key === 'route'}
		<span class="text-muted whitespace-nowrap">{row.route_name}</span>
	{:else if column.key === 'connection'}
		<span class="whitespace-nowrap {row.connection_name ? 'text-muted' : 'problem'}">{connectionOf(row)}</span>
	{:else if column.key === 'error'}
		<span class="problem break-words">{errorOf(row)}</span>
	{:else if column.key === 'attempts'}
		{row.attempt_count}
	{:else}
		<StateTag state={row.state} />
	{/if}
{/snippet}

<div class="failures">
	<DataTable
		rows={listed}
		{columns}
		key={(row) => row.id}
		noun="failures"
		singular="failure"
		{cell}
		search={(row) => `${row.event_title} ${row.route_name} ${connectionOf(row)} ${errorOf(row)}`}
		searchPlaceholder="Search event, route, connection or error"
		initialSort={{ key: 'failed', descending: true }}
		{loaded}
	>
		{#snippet toolbar(found: FailureRow[])}
			<label class="sr-only" for="failures-error">What went wrong</label>
			<select id="failures-error" class="input w-auto max-w-full min-w-0 flex-[0_1_18rem]" bind:value={error}>
				<option value="">Any error</option>
				{#each groups as group (group.error)}
					<option value={group.error}>{group.error} ({group.rows.length})</option>
				{/each}
			</select>
			<label class="flex items-center gap-2 whitespace-nowrap">
				<input type="checkbox" checked={allChosen(found)} disabled={found.length === 0} onchange={() => toggleAll(found)} />
				Select all {found.length}
			</label>
			<button type="button" class="btn btn-primary" disabled={chosen.length === 0 || busy} onclick={() => replay(chosen)}>
				Send {chosen.length || ''} selected again
			</button>
		{/snippet}
		{#snippet empty()}
			<span class="text-ok">Nothing has failed. Every delivery either arrived or is still being retried.</span>
		{/snippet}
	</DataTable>
</div>

<style>
	/* The table draws its own cells, so they are reached with `:global`: the box to tick keeps close to its row. */
	.failures :global(:is(th, td):first-child) {
		padding-right: 0;
	}

	.failures :global(td) {
		vertical-align: top;
	}

	.failures :global(td:first-child) {
		padding-top: 0.85rem;
	}

	@media (width < 48rem) {
		.failures :global(:is(th, td)) {
			padding-right: 0.75rem;
			padding-left: 0.75rem;
		}
	}
</style>
