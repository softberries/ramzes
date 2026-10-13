<script lang="ts">
	import { page } from '$app/state';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { Source, SourceCreated, SourceRequest } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import CopyBlock from '#lib/components/CopyBlock.svelte';
	import { ago, clock } from '#lib/format.ts';
	import { curlForEvent } from '#lib/snippets.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let sources = $state<Source[]>([]);
	let loaded = $state(false);
	let name = $state('');
	let problem = $state('');
	let busy = $state(false);
	/** A key that was just created or rotated. It cannot be shown again. */
	let revealed = $state<{ sourceName: string; key: string } | null>(null);
	let deleting = $state<Source | null>(null);
	let rotating = $state<Source | null>(null);
	/** The source whose rejected requests are shown, with those requests. */
	let inspected = $state<{ id: string; requests: SourceRequest[] } | null>(null);

	async function load() {
		sources = await api<Source[]>('GET', '/api/sources');
		loaded = true;
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
	});

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		problem = '';
		try {
			const created = await api<SourceCreated>('POST', '/api/sources', { name });
			revealed = { sourceName: created.name, key: created.key };
			name = '';
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function rotate() {
		if (!rotating) return;
		try {
			const rotated = await api<{ key: string }>('POST', `/api/sources/${rotating.id}/rotate`);
			revealed = { sourceName: rotating.name, key: rotated.key };
			rotating = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/sources/${deleting.id}`);
			toasts.ok('Source deleted.');
			deleting = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function inspect(source: Source) {
		if (inspected?.id === source.id) {
			inspected = null;
			return;
		}
		try {
			const requests = await api<SourceRequest[]>('GET', `/api/sources/${source.id}/requests`);
			inspected = { id: source.id, requests };
		} catch (error) {
			toasts.problem(error);
		}
	}
</script>

<h1 class="mb-2">Sources</h1>
<p class="hint mb-5 max-w-2xl">
	A source is a service that sends events. Each has its own key, so you can tell senders apart and
	cut one off without touching the others.
</p>

{#if revealed}
	<div class="panel border-gold mb-6 flex flex-col gap-3 p-4">
		<h2>Key for {revealed.sourceName}</h2>
		<p>Copy it now. It is stored only as a hash and cannot be shown again.</p>
		<CopyBlock text={revealed.key} testid="source-key" />
		<p class="text-muted">Send a first event with it:</p>
		<CopyBlock text={curlForEvent(page.url.origin, revealed.key)} />
		<div><button type="button" class="btn" onclick={() => (revealed = null)}>I have saved the key</button></div>
	</div>
{/if}

{#if loaded && sources.length === 0}
	<p class="hint pb-4">No sources yet. Create one to get a key a service can send events with.</p>
{/if}

<ul class="border-line border-t">
	{#each sources as source (source.id)}
		<li class="border-line border-b px-2 py-2.5">
			<div class="grid grid-cols-[1fr_auto] items-baseline gap-x-3">
				<div class="flex min-w-0 flex-wrap items-baseline gap-x-3">
					<span class="font-medium">{source.name}</span>
					<span class="text-muted">{source.key_prefix}…</span>
					<span class="text-faint">created {ago(source.created_at)}</span>
				</div>
				<div class="flex flex-wrap justify-end gap-1">
					<button type="button" class="btn h-7 px-2" aria-expanded={inspected?.id === source.id} onclick={() => inspect(source)}>
						Rejected requests
					</button>
					<button type="button" class="btn h-7 px-2" onclick={() => (rotating = source)}>Replace key</button>
					<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (deleting = source)}>Delete</button>
				</div>
			</div>
			{#if inspected?.id === source.id}
				<div class="mt-3 flex flex-col gap-3">
					{#each inspected.requests as request (request.id)}
						<div>
							<div class="flex flex-wrap gap-x-4">
								<time class="text-muted">{clock(request.received_at)}</time>
								<span class="problem">{request.error}</span>
							</div>
							<pre class="code mt-1 max-h-40 overflow-auto">{request.body}</pre>
						</div>
					{:else}
						<p class="hint">ramzes has not rejected any request from {source.name}.</p>
					{/each}
				</div>
			{/if}
		</li>
	{/each}
</ul>

<form class="mt-8 flex max-w-md flex-col gap-3" onsubmit={create}>
	<h2>Add a source</h2>
	<div>
		<label class="field-label" for="source-name">Name of the service</label>
		<input id="source-name" class="input" required maxlength="100" placeholder="orders-api" bind:value={name} />
	</div>
	{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
	<div><button type="submit" class="btn btn-primary" disabled={busy}>Create source</button></div>
</form>

<Confirm
	open={rotating !== null}
	title="Replace the key of {rotating?.name ?? 'this source'}?"
	action="Replace key"
	onconfirm={rotate}
	oncancel={() => (rotating = null)}
>
	The current key stops working immediately. The service will be rejected until it uses the new one.
</Confirm>

<Confirm
	open={deleting !== null}
	title="Delete {deleting?.name ?? 'this source'}?"
	action="Delete source"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	Its key stops working. Events it already sent are kept.
</Confirm>
