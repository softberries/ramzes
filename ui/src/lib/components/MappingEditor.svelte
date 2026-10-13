<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { MappingPreview, Source, SourceRequest } from '#lib/api/types.ts';
	import { debounced } from '#lib/debounce.ts';
	import { clock } from '#lib/format.ts';
	import { headerList } from '#lib/sources.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import SeverityMark from './SeverityMark.svelte';

	/** How long typing must pause before the preview is asked for again. */
	const PREVIEW_DELAY_MS = 300;
	/** The choice that means "the body pasted below". */
	const PASTED = 'pasted';

	interface Props {
		source: Source;
		/** The source's recent requests, to try the mapping on. */
		requests: SourceRequest[];
		/** Called with the source as stored after its mapping was saved. */
		onsaved: (source: Source) => void;
	}

	let { source, requests, onsaved }: Props = $props();

	// svelte-ignore state_referenced_locally
	let mapping = $state(source.mapping ?? '');
	/** A request id, or `PASTED`. */
	// svelte-ignore state_referenced_locally
	let choice = $state(requests[0]?.id ?? PASTED);
	let pasted = $state('{\n  "title": "Order 17 failed",\n  "severity": "error"\n}');
	let preview = $state<MappingPreview | null>(null);
	let previewProblem = $state('');
	let problem = $state('');
	let busy = $state(false);

	const chosen = $derived(requests.find((request) => request.id === choice));

	/** Counts preview requests, so a slow answer never replaces a newer one. */
	let asked = 0;

	async function askPreview() {
		const mine = ++asked;
		try {
			const answer = await api<MappingPreview>('POST', '/api/sources/preview-mapping', {
				mapping,
				body: chosen ? chosen.body : pasted,
				headers: chosen ? Object.fromEntries(headerList(chosen.headers)) : {}
			});
			if (mine !== asked) return;
			preview = answer;
			previewProblem = '';
		} catch (error) {
			if (mine !== asked) return;
			previewProblem = messageOf(error);
		}
	}

	const askSoon = debounced(askPreview, PREVIEW_DELAY_MS);

	$effect(() => {
		// Everything the preview depends on.
		void [mapping, choice, pasted];
		askSoon();
		return askSoon.cancel;
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		problem = '';
		try {
			const saved = await api<Source>('PUT', `/api/sources/${source.id}/mapping`, { mapping });
			toasts.ok('Mapping saved.');
			onsaved(saved);
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}
</script>

<div class="grid items-start gap-6 xl:grid-cols-2">
	<form class="flex min-w-0 flex-col gap-3" onsubmit={save}>
		<div>
			<label class="field-label" for="mapping-{source.id}">Mapping</label>
			<textarea
				id="mapping-{source.id}"
				class="input h-72 py-2 whitespace-pre"
				spellcheck="false"
				required
				bind:value={mapping}
			></textarea>
			<p class="hint mt-1">
				Each field is an expression over the request: payload is its JSON body, headers its headers by
				lower-case name. Text goes in quotes, as in 'shop.' ~ payload.kind.
			</p>
		</div>
		{#if problem}<p class="problem break-words" role="alert">{problem}</p>{/if}
		<div><button type="submit" class="btn btn-primary" disabled={busy}>Save mapping</button></div>
	</form>

	<section class="flex min-w-0 flex-col gap-3" aria-label="Mapping preview">
		<div>
			<label class="field-label" for="mapping-request-{source.id}">Try it on</label>
			<select id="mapping-request-{source.id}" class="input" bind:value={choice}>
				{#each requests as request (request.id)}
					<option value={request.id}>
						Request at {clock(request.received_at)}{request.error ? ', rejected' : ''}
					</option>
				{/each}
				<option value={PASTED}>A body I paste here</option>
			</select>
		</div>
		{#if chosen}
			<pre class="code max-h-40 overflow-auto">{chosen.body}</pre>
		{:else}
			<div>
				<label class="field-label" for="mapping-body-{source.id}">Request body</label>
				<textarea
					id="mapping-body-{source.id}"
					class="input h-32 py-2"
					spellcheck="false"
					bind:value={pasted}
				></textarea>
			</div>
		{/if}

		{#if previewProblem}
			<p class="problem" role="alert">{previewProblem}</p>
		{:else if preview?.error}
			<p class="problem break-words" role="alert">{preview.error}</p>
		{:else if preview && preview.events.length === 0}
			<p class="hint">This request would be accepted without making an event.</p>
		{:else if preview}
			<div>
				<h3 class="field-label">
					{preview.events.length === 1 ? 'The event it makes' : `The ${preview.events.length} events it makes`}
				</h3>
				<ul class="border-line border-t" aria-label="Events the mapping makes">
					{#each preview.events as event, index (index)}
						<li class="border-line border-b py-2">
							<div class="font-medium break-words">{event.title}</div>
							<div class="text-muted flex flex-wrap gap-x-4">
								<span class="break-all">{event.topic}</span>
								<SeverityMark severity={event.severity ?? 'info'} label />
								<span>{event.status ?? 'firing'}</span>
							</div>
							{#if event.body}<p class="text-muted break-words whitespace-pre-wrap">{event.body}</p>{/if}
							{#if Object.keys(event.labels ?? {}).length > 0}
								<div class="mt-1 flex flex-wrap gap-1">
									{#each Object.entries(event.labels ?? {}) as [key, value] (key)}
										<span class="tag max-w-full truncate">{key}={value}</span>
									{/each}
								</div>
							{/if}
						</li>
					{/each}
				</ul>
			</div>
		{/if}
	</section>
</div>
