<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { EventPage, EventRow, RouteInput, RouteResult, RouteTest } from '#lib/api/types.ts';
	import { debounced } from '#lib/debounce.ts';
	import { clock } from '#lib/format.ts';
	import { SAMPLE_EVENT, outcomeWords, parseSampleEvent } from '#lib/routes.ts';
	import { outcomeTone, stepWords } from '#lib/shaping.ts';

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

	/** The colour of what noise control would do with a target. */
	const TONES = { sends: 'var(--st-delivered)', waits: 'var(--gold)', quiet: 'var(--muted)' } as const;
</script>

<section class="card overflow-hidden" aria-label="Try an event">
	<div class="card-head">
		<div>
			<h2>Try an event</h2>
			<p class="hint">
				What these routes would do with one event, including changes you have not saved. Nothing is sent.
			</p>
		</div>
	</div>
	<div class="card-body flex flex-col gap-3">
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
		{/if}
	</div>

	{#if !problem && results && results.length > 0}
		<ol class="results border-line border-t" aria-label="Result for each route">
			{#each results as result, index (index)}
				<li class="grid grid-cols-[1.25rem_minmax(0,1fr)] gap-x-2 px-[1.1rem] py-2.5" data-outcome={result.outcome}>
					<span class="text-faint tabular-nums">{index + 1}</span>
					<div class="min-w-0">
						<div class="flex flex-wrap items-center gap-x-2.5 gap-y-1">
							<span class="font-medium break-words">{result.name}</span>
							{#if index === draftIndex}<span class="tag">being edited</span>{/if}
						</div>
						{#if result.outcome === 'matched'}
							<span class="badge mt-1" style="--tone: var(--st-delivered)">{outcomeWords(result)}</span>
						{:else if result.outcome === 'not_matched' || result.outcome === 'disabled'}
							<span class="badge mt-1">{outcomeWords(result)}</span>
						{:else}
							<div class="{result.outcome === 'error' ? 'problem' : 'text-muted'} break-words">{outcomeWords(result)}</div>
						{/if}
						{#each result.targets as target, i (i)}
							{@const explained = target.steps.filter((step) => step.step !== 'send')}
							<div class="target mt-2 break-words" style="--tone: {TONES[outcomeTone(target.outcome)]}" data-testid="target">
								<div class="outcome font-medium" data-testid="target-outcome">
									{target.outcome}
								</div>
								<span class="text-muted">
									to
									<span class={target.connection_name ? 'text-fg' : 'problem'}
										>{target.connection_name ?? 'a deleted connection'}</span
									>{target.destination ? ` (${target.destination})` : ''}:
								</span>
								{target.title}
								{#if target.warning}<div class="text-gold">{target.warning}</div>{/if}
								{#if explained.length > 0}
									<!-- Not a list: the rows of the result list are counted by their list items. -->
									<div class="text-muted" data-testid="target-steps">
										{#each explained as step, at (at)}
											<div><span class="text-faint">{stepWords(step.step)}:</span> {step.outcome}</div>
										{/each}
									</div>
								{/if}
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

<style>
	.results > li + li {
		border-top: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
	}

	/* One message that would go out, tinted with what noise control would do with it. */
	.target {
		padding: 0.5rem 0.65rem;
		border-radius: var(--radius-control);
		background: color-mix(in srgb, var(--tone) 9%, transparent);
	}

	.outcome {
		color: var(--tone);
	}
</style>
