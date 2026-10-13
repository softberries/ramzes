<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { RouteMatch, SilenceRow } from '#lib/api/types.ts';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import MatchEditor from '#lib/components/MatchEditor.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import Segments from '#lib/components/Segments.svelte';
	import { clock } from '#lib/format.ts';
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

	/** Whether a silence applies now, later or no longer; the list shows one of these at a time. */
	type Kind = 'active' | 'upcoming' | 'ended';

	const KINDS: { key: Kind; label: string; when: string; none: string }[] = [
		{ key: 'active', label: 'Active', when: 'Ends', none: 'No silence is active. Everything that matches a route is sent.' },
		{ key: 'upcoming', label: 'Upcoming', when: 'Starts', none: 'No silence is set to start later.' },
		{ key: 'ended', label: 'Ended', when: 'Ended', none: 'No silence has ended yet.' }
	];

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

	let kind = $state<Kind>('active');

	const groups = $derived(groupSilences(silences, now));
	const shown = $derived(KINDS.find((k) => k.key === kind) ?? KINDS[0]);

	const matchOf = (silence: SilenceRow) => summarize(silence.match, lookups.sourceNames);

	const columns = $derived<Column<SilenceRow>[]>([
		{ key: 'match', label: 'Events silenced', sort: matchOf },
		{
			key: 'when',
			label: shown.when,
			sort: (silence) => (kind === 'upcoming' ? silence.starts_at : silence.ends_at),
			descendingFirst: kind === 'ended',
			from: 'md'
		},
		{ key: 'author', label: 'Created by', sort: (silence) => silence.author, from: 'lg' },
		{ key: 'created', label: 'Created', sort: (silence) => silence.created_at, descendingFirst: true, from: 'lg' },
		{ key: 'actions', label: 'Actions', end: true }
	]);

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
			kind = 'active';
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

<PageHeader title="Silences">
	While a silence is active, the events it matches are still recorded in the feed but sent nowhere. Use one for a
	deploy, a maintenance window, or an alert that is already being handled.
	{#snippet actions()}
		<button type="button" class="btn btn-primary" onclick={() => start()}><Icon name="plus" />New silence</button>
	{/snippet}
</PageHeader>

{#if draft}
	<form class="card mb-5" onsubmit={create} aria-label="New silence">
		<div class="card-head"><h2>New silence</h2></div>
		<div class="card-body flex flex-col gap-6">
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
				<div class="flex flex-wrap gap-1.5" role="group" aria-label="For how long">
					{#each DURATIONS as choice (choice.seconds)}
						<button
							type="button"
							class="btn {draft.seconds === choice.seconds ? 'chosen' : 'text-muted'}"
							aria-pressed={draft.seconds === choice.seconds}
							onclick={() => draft && (draft.seconds = choice.seconds)}
						>
							{choice.label}
						</button>
					{/each}
					<button
						type="button"
						class="btn {draft.seconds === UNTIL ? 'chosen' : 'text-muted'}"
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
					placeholder="Why these events are silenced"
					bind:value={draft.comment}
				/>
			</div>

			{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
		</div>
		<div class="border-line flex gap-2 border-t px-[1.1rem] py-3">
			<button type="submit" class="btn btn-primary" disabled={busy}>Create silence</button>
			<button type="button" class="btn" onclick={() => (draft = null)}>Cancel</button>
		</div>
	</form>
{/if}

{#snippet cell(silence: SilenceRow, column: Column<SilenceRow>)}
	{#if column.key === 'match'}
		<div class="min-w-0" data-kind={kind}>
			<div class="font-medium break-words {kind === 'ended' ? 'text-muted' : ''}">{matchOf(silence)}</div>
			{#if silence.comment}<div class="text-muted break-words">{silence.comment}</div>{/if}
			<!-- When, and who made it, are columns of their own on a wider screen. -->
			<div class="break-words md:hidden {kind === 'active' ? 'text-gold' : 'text-muted'}">{silenceWhen(silence, now)}</div>
			<div class="text-faint break-all lg:hidden">by {silence.author}</div>
		</div>
	{:else if column.key === 'when'}
		<span class="break-words {kind === 'active' ? 'text-gold' : 'text-muted'}">{silenceWhen(silence, now)}</span>
	{:else if column.key === 'author'}
		<span class="text-muted break-all">{silence.author}</span>
	{:else if column.key === 'created'}
		<time class="text-muted whitespace-nowrap">{clock(silence.created_at, now)}</time>
	{:else if kind === 'active'}
		<button type="button" class="btn btn-sm max-md:h-10 max-md:px-3" onclick={() => endNow(silence)}>End now</button>
	{:else}
		<button type="button" class="btn btn-sm btn-danger max-md:h-10 max-md:px-3" onclick={() => remove(silence)}>
			Delete
		</button>
	{/if}
{/snippet}

{#if loaded && silences.length === 0 && !draft}
	<p class="card hint p-6">
		No silences, so every event that matches a route is sent.
		<button type="button" class="text-accent underline" onclick={() => start()}>Create a silence</button>
		before the next deploy.
	</p>
{:else if loaded}
	<section class="silences" aria-label="{shown.label} silences">
		<!-- Each of the three starts in its own order: ending soonest, starting soonest, ended last. -->
		{#key kind}
			<DataTable
				rows={groups[kind]}
				{columns}
				key={(silence) => silence.id}
				noun="{kind} silences"
				singular="{kind} silence"
				{cell}
				search={(silence) => `${matchOf(silence)} ${silence.comment ?? ''} ${silence.author}`}
				searchPlaceholder="Search conditions, comment or author"
			>
				{#snippet toolbar()}
					<div role="group" aria-label="Silences to show">
						<Segments>
							{#each KINDS as item (item.key)}
								<button type="button" aria-pressed={item.key === kind} onclick={() => (kind = item.key)}>
									{item.label}
									<span class="count">{groups[item.key].length}</span>
								</button>
							{/each}
						</Segments>
					</div>
				{/snippet}
				{#snippet empty()}{shown.none}{/snippet}
			</DataTable>
		{/key}
	</section>
{/if}

<style>
	.chosen {
		border-color: var(--accent);
		background: color-mix(in srgb, var(--accent) 12%, var(--surface));
		color: var(--accent);
	}

	.silences :global(td) {
		vertical-align: top;
	}

	@media (width < 48rem) {
		.silences :global(:is(th, td)) {
			padding-right: 0.75rem;
			padding-left: 0.75rem;
		}
	}
</style>
