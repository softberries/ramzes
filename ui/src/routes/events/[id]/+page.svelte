<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { ApiError, api } from '#lib/api/client.ts';
	import type { Journey, MessagePreview } from '#lib/api/types.ts';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import Timeline from '#lib/components/Timeline.svelte';
	import { clock } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const id = $derived(page.params.id ?? '');

	let journey = $state<Journey | null>(null);
	/** The message each notification carries, by notification id. */
	let previews = $state<Record<string, MessagePreview>>({});
	let missing = $state(false);
	let now = $state(Date.now());
	let busy = $state(false);

	async function load() {
		try {
			const [loaded, messages] = await Promise.all([
				api<Journey>('GET', `/api/events/${id}`),
				// The journey is still worth showing when the messages cannot be worded.
				api<MessagePreview[]>('GET', `/api/events/${id}/preview`).catch((error) => {
					if (!(error instanceof ApiError && error.status === 404)) toasts.problem(error);
					return [];
				})
			]);
			journey = loaded;
			previews = Object.fromEntries(messages.map((message) => [message.notification_id, message]));
			missing = false;
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) missing = true;
			else toasts.problem(error);
		}
	}

	$effect(() => {
		void id;
		journey = null;
		previews = {};
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

<p class="text-muted mb-3"><a href={resolve('/')}>Feed</a> <span class="text-faint">/</span> Event</p>

{#if missing}
	<h1>This event does not exist</h1>
	<p class="hint mt-2">
		It may have been removed after its retention period. See the ones that are left in the
		<a href={resolve('/')}>Feed</a>.
	</p>
{:else if journey}
	{@const event = journey.event}
	<header class="mb-5 flex flex-wrap items-start gap-x-6 gap-y-3">
		<div class="mr-auto min-w-0">
			<h1 class="break-words">{event.title}</h1>
			<div class="text-muted mt-1 flex flex-wrap items-baseline gap-x-5 gap-y-1">
				<SeverityMark severity={event.severity} label />
				<span class="mono break-all">{event.topic}</span>
				{#if event.status === 'resolved'}<span class="tag">resolved</span>{/if}
				<time>{clock(event.received_at, now)}</time>
			</div>
		</div>
		<button type="button" class="btn" disabled={busy} onclick={replayEvent}>Run through routes again</button>
	</header>

	<section class="card mb-4" aria-label="What happened">
		<div class="card-head"><h2>What happened</h2></div>
		<div class="card-body">
			<Timeline
				{journey}
				connections={lookups.connectionNames}
				sources={lookups.sourceNames}
				{previews}
				{now}
				onreplay={replayDelivery}
			/>
		</div>
	</section>

	<section class="grid items-start gap-4 xl:grid-cols-2" aria-label="Event content">
		<div class="flex min-w-0 flex-col gap-4">
			<div class="card">
				<div class="card-head"><h2>Details</h2></div>
				<dl class="card-body text-muted grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5">
					<dt>Event id</dt>
					<dd class="mono text-fg break-all">{event.id}</dd>
					<dt>Occurred</dt>
					<dd class="text-fg"><time>{clock(event.occurred_at, now)}</time></dd>
					{#if event.dedup_key}
						<dt>Dedup key</dt>
						<dd class="mono text-fg break-all">{event.dedup_key}</dd>
					{/if}
				</dl>
			</div>
			{#if event.body}
				<div class="card">
					<div class="card-head"><h2>Body</h2></div>
					<p class="card-body break-words whitespace-pre-wrap">{event.body}</p>
				</div>
			{/if}
			<div class="card overflow-hidden">
				<div class="card-head"><h2>Labels</h2></div>
				{#if Object.keys(event.labels).length > 0}
					<table class="table">
						<tbody>
							{#each Object.entries(event.labels) as [key, value] (key)}
								<tr>
									<th scope="row" class="mono w-px">{key}</th>
									<td class="mono break-all">{value}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{:else}
					<p class="card-body hint">This event has no labels.</p>
				{/if}
			</div>
		</div>
		<div class="card min-w-0">
			<div class="card-head"><h2>Payload</h2></div>
			<div class="card-body">
				{#if event.payload === null || event.payload === undefined}
					<p class="hint">This event was sent without a payload.</p>
				{:else}
					<pre class="code max-h-[32rem] overflow-auto">{JSON.stringify(event.payload, null, 2)}</pre>
				{/if}
			</div>
		</div>
	</section>
{/if}
