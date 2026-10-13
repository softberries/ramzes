<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api/client.ts';
	import type { Journey } from '#lib/api/types.ts';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import Timeline from '#lib/components/Timeline.svelte';
	import { clock } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const id = $derived(page.params.id ?? '');

	let journey = $state<Journey | null>(null);
	let missing = $state(false);
	let now = $state(Date.now());
	let busy = $state(false);

	async function load() {
		try {
			journey = await api<Journey>('GET', `/api/events/${id}`);
			missing = false;
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) missing = true;
			else toasts.problem(error);
		}
	}

	$effect(() => {
		void id;
		journey = null;
		load();
		lookups.load().catch((error) => toasts.problem(error));
		const offs = [
			live.on('event', (m) => m.event_id === id && load()),
			live.on('delivery', (m) => m.event_ids.includes(id) && load()),
			live.on('resync', load)
		];
		// Keeps "next attempt in…" current.
		const tick = setInterval(() => (now = Date.now()), 1000);
		return () => {
			offs.forEach((off) => off());
			clearInterval(tick);
		};
	});

	async function act(action: () => Promise<string>) {
		busy = true;
		try {
			toasts.ok(await action());
			await load();
		} catch (error) {
			toasts.problem(error);
		} finally {
			busy = false;
		}
	}

	function replayEvent() {
		act(async () => {
			const result = await api<{ deliveries: number }>('POST', `/api/events/${id}/replay`);
			return result.deliveries === 0
				? 'No route matches this event.'
				: `Queued ${result.deliveries} ${result.deliveries === 1 ? 'delivery' : 'deliveries'}.`;
		});
	}

	function replayDelivery(deliveryId: string) {
		act(async () => {
			await api('POST', `/api/deliveries/${deliveryId}/replay`);
			return 'Queued to send again.';
		});
	}
</script>

<p class="mb-3"><a href={resolve('/')}>Feed</a> <span class="text-faint">/</span> Event</p>

{#if missing}
	<h1>This event does not exist</h1>
	<p class="hint mt-2">It may have been removed after its retention period.</p>
{:else if journey}
	{@const event = journey.event}
	<header class="mb-6 flex flex-wrap items-start gap-x-6 gap-y-3">
		<div class="mr-auto min-w-0">
			<h1 class="break-words">{event.title}</h1>
			<div class="text-muted mt-1 flex flex-wrap gap-x-5 gap-y-1">
				<SeverityMark severity={event.severity} label />
				<span>{event.topic}</span>
				{#if event.status === 'resolved'}<span>resolved</span>{/if}
				<time>{clock(event.received_at, now)}</time>
			</div>
		</div>
		<button type="button" class="btn" disabled={busy} onclick={replayEvent}>Run through routes again</button>
	</header>

	<section class="mb-8" aria-label="What happened">
		<Timeline
			{journey}
			connections={lookups.connectionNames}
			sources={lookups.sourceNames}
			{now}
			onreplay={replayDelivery}
		/>
	</section>

	<section class="grid gap-6 xl:grid-cols-2" aria-label="Event content">
		<div class="flex flex-col gap-4">
			{#if event.body}
				<div>
					<h2 class="mb-1">Message</h2>
					<p class="code">{event.body}</p>
				</div>
			{/if}
			<div>
				<h2 class="mb-1">Labels</h2>
				{#if Object.keys(event.labels).length > 0}
					<table class="code w-full">
						<tbody>
							{#each Object.entries(event.labels) as [key, value] (key)}
								<tr><td class="text-muted pr-6 align-top whitespace-nowrap">{key}</td><td class="break-all">{value}</td></tr>
							{/each}
						</tbody>
					</table>
				{:else}
					<p class="hint">This event has no labels.</p>
				{/if}
			</div>
			<dl class="text-muted grid grid-cols-[auto_1fr] gap-x-6 gap-y-1">
				<dt>Event id</dt>
				<dd class="text-fg break-all">{event.id}</dd>
				<dt>Occurred</dt>
				<dd class="text-fg">{clock(event.occurred_at, now)}</dd>
				{#if event.dedup_key}
					<dt>Dedup key</dt>
					<dd class="text-fg break-all">{event.dedup_key}</dd>
				{/if}
			</dl>
		</div>
		<div>
			<h2 class="mb-1">Payload</h2>
			{#if event.payload === null || event.payload === undefined}
				<p class="hint">This event was sent without a payload.</p>
			{:else}
				<pre class="code max-h-[32rem] overflow-auto">{JSON.stringify(event.payload, null, 2)}</pre>
			{/if}
		</div>
	</section>
{/if}
