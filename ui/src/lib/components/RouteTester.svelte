<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { EventPage, EventRow, RouteInput, RouteResult, RouteTest } from '#lib/api/types.ts';
	import { debounced } from '#lib/debounce.ts';
	import { clock } from '#lib/format.ts';
	import { SAMPLE_EVENT, outcomeWords, parseSampleEvent } from '#lib/routes.ts';

	/** How long editing must pause before the routes are tried again. */
	const TEST_DELAY_MS = 400;
	/** The choice that means "the event written below". */
	const WRITTEN = 'written';

	interface Props {
		/** Every route in evaluation order, including the unsaved draft. Build it with `routesForTest`. */
		routes: RouteInput[];
		/** Where the unsaved draft sits in `routes`, when there is one. */
		draftIndex?: number | null;
	}

	let { routes, draftIndex = null }: Props = $props();

	let events = $state<EventRow[]>([]);
	/** An event id, or `WRITTEN`. */
	let choice = $state(WRITTEN);
	let written = $state(SAMPLE_EVENT);
	let results = $state<RouteResult[] | null>(null);
	let problem = $state('');

	$effect(() => {
		api<EventPage>('GET', '/api/events?limit=20')
			.then((page) => {
				events = page.events;
				// A real event says more than the sample, so start from the newest one.
				if (events.length > 0) choice = events[0].id;
			})
			.catch((error) => (problem = messageOf(error)));
	});

	/** Counts requests, so a slow answer never replaces a newer one. */
	let asked = 0;

	async function run() {
		const mine = ++asked;
		let event: { event_id: string } | { event: unknown };
		if (choice === WRITTEN) {
			const parsed = parseSampleEvent(written);
			if (parsed.problem) {
				problem = parsed.problem;
				results = null;
				return;
			}
			event = { event: parsed.event };
		} else {
			event = { event_id: choice };
		}
		try {
			const answer = await api<RouteTest>('POST', '/api/routes/test', { ...event, routes });
			if (mine !== asked) return;
			results = answer.results;
			problem = '';
		} catch (error) {
			if (mine !== asked) return;
			results = null;
			problem = messageOf(error);
		}
	}

	const runSoon = debounced(run, TEST_DELAY_MS);

	$effect(() => {
		// Everything the answer depends on.
		void [JSON.stringify(routes), choice, written];
		runSoon();
		return runSoon.cancel;
	});

	function tone(result: RouteResult): string {
		if (result.outcome === 'matched') return 'text-ok';
		return result.outcome === 'error' ? 'problem' : 'text-muted';
	}
</script>

<section class="panel flex flex-col gap-3 p-4" aria-label="Try an event">
	<h2>Try an event</h2>
	<p class="hint">
		What these routes would do with one event, including changes you have not saved. Nothing is sent.
	</p>
	<div>
		<label class="field-label" for="tester-event">Event</label>
		<select id="tester-event" class="input" bind:value={choice}>
			<option value={WRITTEN}>An event I write here</option>
			{#each events as event (event.id)}
				<option value={event.id}>{clock(event.received_at)} {event.title}</option>
			{/each}
		</select>
	</div>
	{#if choice === WRITTEN}
		<div>
			<label class="field-label" for="tester-sample">Event as JSON</label>
			<textarea id="tester-sample" class="input h-48 py-2" spellcheck="false" bind:value={written}></textarea>
		</div>
	{/if}

	{#if problem}
		<p class="problem" role="alert">{problem}</p>
	{:else if results && results.length === 0}
		<p class="hint">There are no routes, so this event would not be sent anywhere.</p>
	{:else if results}
		<ol class="border-line border-t" aria-label="Result for each route">
			{#each results as result, index (index)}
				<li class="border-line grid grid-cols-[1.5rem_1fr] gap-x-2 border-b py-2" data-outcome={result.outcome}>
					<span class="text-faint">{index + 1}</span>
					<div class="min-w-0">
						<div class="flex flex-wrap items-baseline gap-x-3">
							<span class="font-medium break-words">{result.name}</span>
							{#if index === draftIndex}<span class="tag">being edited</span>{/if}
						</div>
						<div class="{tone(result)} break-words">{outcomeWords(result)}</div>
						{#each result.targets as target, i (i)}
							<div class="mt-1 break-words">
								<span class="text-muted">
									to
									<span class={target.connection_name ? 'text-fg' : 'problem'}
										>{target.connection_name ?? 'a deleted connection'}</span
									>{target.destination ? ` (${target.destination})` : ''}:
								</span>
								{target.title}
								{#if target.warning}<div class="text-gold">{target.warning}</div>{/if}
							</div>
						{/each}
						{#if result.outcome === 'matched' && result.targets.length === 0}
							<div class="hint">It has no connection to send to.</div>
						{/if}
					</div>
				</li>
			{/each}
		</ol>
	{/if}
</section>
