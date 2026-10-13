<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { RouteMatch, SilenceRow } from '#lib/api/types.ts';
	import MatchEditor from '#lib/components/MatchEditor.svelte';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { cleanMatch, summarize } from '#lib/routes.ts';
	import {
		DURATIONS,
		groupSilences,
		localInput,
		matchFromQuery,
		newSilence,
		silenceWhen,
		type SilenceLength
	} from '#lib/silences.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** The choice that means "until a date and time". */
	const UNTIL = 0;
	const DAY_MS = 86_400_000;
	/** How often the lists are sorted again, so a silence that ends moves by itself. */
	const TICK_MS = 15_000;

	/** The silence being created. */
	interface Draft {
		match: RouteMatch;
		/** A number of seconds from `DURATIONS`, or `UNTIL`. */
		seconds: number;
		/** The end as the date-and-time field holds it, used with `UNTIL`. */
		until: string;
		comment: string;
	}

	let silences = $state<SilenceRow[]>([]);
	let loaded = $state(false);
	let draft = $state<Draft | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let now = $state(Date.now());

	const groups = $derived(groupSilences(silences, now));

	async function load() {
		silences = await api<SilenceRow[]>('GET', '/api/silences');
		now = Date.now();
		loaded = true;
	}

	function start(match: RouteMatch = {}) {
		problem = '';
		draft = { match, seconds: DURATIONS[1].seconds, until: localInput(Date.now() + DAY_MS), comment: '' };
	}

	$effect(() => {
		Promise.all([load(), lookups.load()]).catch((error) => toasts.problem(error));
		const tick = setInterval(() => (now = Date.now()), TICK_MS);
		return () => clearInterval(tick);
	});

	// "Silence like this" arrives with the conditions in the address.
	$effect(() => {
		const asked = matchFromQuery(page.url.searchParams);
		if (Object.keys(asked).length > 0) start(asked);
	});

	async function create(event: SubmitEvent) {
		event.preventDefault();
		if (!draft) return;
		const length: SilenceLength = draft.seconds === UNTIL ? { until: draft.until } : { seconds: draft.seconds };
		const made = newSilence(draft.match, length, draft.comment, Date.now());
		if ('problem' in made) {
			problem = made.problem;
			return;
		}
		busy = true;
		problem = '';
		try {
			await api('POST', '/api/silences', made.body);
			toasts.ok('Silence created.');
			draft = null;
			// Leave the prefilled address behind, so a reload does not open the form again.
			if (page.url.search) await goto(resolve('/silences'), { replace: true });
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function act(action: () => Promise<unknown>, done: string) {
		try {
			await action();
			toasts.ok(done);
		} catch (error) {
			toasts.problem(error);
		}
		await load().catch((error) => toasts.problem(error));
	}

	function endNow(silence: SilenceRow) {
		act(() => api('POST', `/api/silences/${silence.id}/end`), 'Silence ended. Matching events are sent again.');
	}

	function remove(silence: SilenceRow) {
		act(() => api('DELETE', `/api/silences/${silence.id}`), 'Silence deleted.');
	}
</script>

{#snippet row(silence: SilenceRow, kind: 'active' | 'upcoming' | 'ended')}
	<li class="border-line grid items-baseline gap-x-3 gap-y-2 border-b px-2 py-2.5 md:grid-cols-[1fr_auto]" data-kind={kind}>
		<div class="min-w-0">
			<div class="font-medium break-words {kind === 'ended' ? 'text-muted' : ''}">
				{summarize(silence.match, lookups.sourceNames)}
			</div>
			{#if silence.comment}<div class="break-words">{silence.comment}</div>{/if}
			<div class="text-muted break-words">
				<span class={kind === 'active' ? 'text-gold' : ''}>{silenceWhen(silence, now)}</span>
				<span class="text-faint">by {silence.author}</span>
			</div>
		</div>
		<div class="flex flex-wrap gap-1 md:justify-end">
			{#if kind === 'active'}
				<button type="button" class="btn h-10 px-3 md:h-7 md:px-2" onclick={() => endNow(silence)}>End now</button>
			{:else}
				<button type="button" class="btn btn-danger h-10 px-3 md:h-7 md:px-2" onclick={() => remove(silence)}>
					Delete
				</button>
			{/if}
		</div>
	</li>
{/snippet}

<div class="mb-2 flex items-center gap-3">
	<h1 class="mr-auto">Silences</h1>
	<button type="button" class="btn btn-primary" onclick={() => start()}>New silence</button>
</div>
<p class="hint mb-5 max-w-2xl">
	While a silence is active, the events it matches are still recorded in the feed but sent nowhere. Use one for a
	deploy, a maintenance window, or an alert that is already being handled.
</p>

{#if draft}
	<form class="panel mb-8 flex flex-col gap-5 p-4" onsubmit={create} aria-label="New silence">
		<h2>New silence</h2>
		<fieldset class="flex flex-col gap-3">
			<legend class="mb-2 font-medium">Which events</legend>
			<MatchEditor bind:match={draft.match} sources={lookups.sources} />
			<p class="text-muted break-words">
				{#if Object.keys(cleanMatch(draft.match)).length === 0}
					Add at least one condition: a silence cannot cover every event.
				{:else}
					Silences: {summarize(cleanMatch(draft.match), lookups.sourceNames)}
				{/if}
			</p>
		</fieldset>

		<fieldset class="flex flex-col gap-3">
			<legend class="mb-2 font-medium">For how long</legend>
			<div class="flex flex-wrap gap-1" role="group" aria-label="For how long">
				{#each DURATIONS as choice (choice.seconds)}
					<button
						type="button"
						class="btn {draft.seconds === choice.seconds ? 'border-accent text-accent' : 'text-muted'}"
						aria-pressed={draft.seconds === choice.seconds}
						onclick={() => draft && (draft.seconds = choice.seconds)}
					>
						{choice.label}
					</button>
				{/each}
				<button
					type="button"
					class="btn {draft.seconds === UNTIL ? 'border-accent text-accent' : 'text-muted'}"
					aria-pressed={draft.seconds === UNTIL}
					onclick={() => draft && (draft.seconds = UNTIL)}
				>
					Until a date and time
				</button>
			</div>
			{#if draft.seconds === UNTIL}
				<div class="max-w-xs">
					<label class="field-label" for="silence-until">Ends at</label>
					<input id="silence-until" class="input" type="datetime-local" required bind:value={draft.until} />
				</div>
			{/if}
		</fieldset>

		<div class="max-w-xl">
			<label class="field-label" for="silence-comment">Comment</label>
			<input
				id="silence-comment"
				class="input"
				maxlength="500"
				placeholder="Why, for whoever wonders where the messages went"
				bind:value={draft.comment}
			/>
		</div>

		{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
		<div class="flex gap-2">
			<button type="submit" class="btn btn-primary" disabled={busy}>Create silence</button>
			<button type="button" class="btn" onclick={() => (draft = null)}>Cancel</button>
		</div>
	</form>
{/if}

{#if loaded && silences.length === 0 && !draft}
	<p class="hint py-8">
		No silences, so every event that matches a route is sent.
		<button type="button" class="text-accent underline" onclick={() => start()}>Create a silence</button>
		before the next deploy.
	</p>
{:else if loaded}
	<section class="mb-8" aria-label="Active silences">
		<h2 class="mb-2">Active</h2>
		<ul class="border-line border-t">
			{#each groups.active as silence (silence.id)}
				{@render row(silence, 'active')}
			{:else}
				<li class="hint border-line border-b px-2 py-2">No silence is active. Everything that matches a route is sent.</li>
			{/each}
		</ul>
	</section>

	{#if groups.upcoming.length > 0}
		<section class="mb-8" aria-label="Upcoming silences">
			<h2 class="mb-2">Upcoming</h2>
			<ul class="border-line border-t">
				{#each groups.upcoming as silence (silence.id)}
					{@render row(silence, 'upcoming')}
				{/each}
			</ul>
		</section>
	{/if}

	{#if groups.ended.length > 0}
		<details aria-label="Ended silences">
			<summary class="cursor-pointer font-semibold">Ended ({groups.ended.length})</summary>
			<ul class="border-line mt-2 border-t">
				{#each groups.ended as silence (silence.id)}
					{@render row(silence, 'ended')}
				{/each}
			</ul>
		</details>
	{/if}
{/if}
