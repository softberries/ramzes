<script lang="ts">
	import { resolve } from '$app/paths';
	import { api } from '#lib/api/client.ts';
	import type { FailureRow } from '#lib/api/types.ts';
	import StateTag from '#lib/components/StateTag.svelte';
	import { groupByError } from '#lib/failures.ts';
	import { clock } from '#lib/format.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let failures = $state<FailureRow[]>([]);
	let loaded = $state(false);
	let chosen = $state<string[]>([]);
	let busy = $state(false);

	const groups = $derived(groupByError(failures));

	async function load() {
		failures = await api<FailureRow[]>('GET', '/api/failures');
		// Forget selections of failures that are no longer listed.
		chosen = chosen.filter((id) => failures.some((f) => f.id === id));
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

	function toggleGroup(ids: string[]) {
		const all = ids.every((id) => chosen.includes(id));
		chosen = all ? chosen.filter((id) => !ids.includes(id)) : [...new Set([...chosen, ...ids])];
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
</script>

<div class="mb-2 flex flex-wrap items-center gap-3">
	<h1 class="mr-auto">Failures</h1>
	<button type="button" class="btn btn-primary" disabled={chosen.length === 0 || busy} onclick={() => replay(chosen)}>
		Send {chosen.length || ''} selected again
	</button>
</div>
<p class="hint mb-5 max-w-2xl">
	Deliveries that were rejected or ran out of retries, grouped by what went wrong. Sending one again
	leaves the failed attempt in the event's history.
</p>

{#if loaded && failures.length === 0}
	<p class="text-ok py-8">Nothing has failed. Every delivery either arrived or is still being retried.</p>
{/if}

{#each groups as group (group.error)}
	{@const ids = group.rows.map((row) => row.id)}
	<section class="mb-6">
		<div class="border-line flex flex-wrap items-baseline gap-x-4 border-b pb-1.5">
			<label class="flex min-w-0 items-baseline gap-2">
				<input
					type="checkbox"
					checked={ids.every((id) => chosen.includes(id))}
					onchange={() => toggleGroup(ids)}
					aria-label="Select all {group.rows.length} with this error"
				/>
				<span class="problem font-medium break-words">{group.error}</span>
			</label>
			<span class="text-muted">{group.rows.length} {group.rows.length === 1 ? 'delivery' : 'deliveries'}</span>
		</div>
		<ul>
			{#each group.rows as row (row.id)}
				<li class="border-line grid grid-cols-[auto_5.5rem_1fr_auto] items-baseline gap-x-3 border-b px-2 py-1.5">
					<input
						type="checkbox"
						checked={chosen.includes(row.id)}
						onchange={() => toggle(row.id)}
						aria-label="Select {row.event_title}"
					/>
					<time class="text-muted whitespace-nowrap">{clock(row.updated_at)}</time>
					<span class="min-w-0 truncate">
						<a href={resolve('/events/[id]', { id: row.event_id })}>{row.event_title}</a>
						<span class="text-muted ml-2">
							{row.route_name} to {row.connection_name ?? 'a deleted connection'}
						</span>
					</span>
					<span class="text-right"><StateTag state={row.state} /></span>
				</li>
			{/each}
		</ul>
	</section>
{/each}
