<script lang="ts">
	import { page } from '$app/state';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { Source, SourceCreated, SourceKind, SourceRequest } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import CopyBlock from '#lib/components/CopyBlock.svelte';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import MappingEditor from '#lib/components/MappingEditor.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { ago, clock } from '#lib/format.ts';
	import { reveal } from '#lib/reveal.ts';
	import { headerList, isBuiltIn, setupText } from '#lib/sources.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	const NATIVE = 'native';
	const JSON_KIND = 'json';

	let sources = $state<Source[]>([]);
	let kinds = $state<SourceKind[]>([]);
	let loaded = $state(false);

	/** The new source's form. */
	let kind = $state(NATIVE);
	let name = $state('');
	let ownSecret = $state('');
	let problem = $state('');
	let busy = $state(false);

	/** A source whose key was just set. The key cannot be shown again. */
	let revealed = $state<{ source: Source; key: string } | null>(null);
	let deleting = $state<Source | null>(null);
	let rotating = $state<Source | null>(null);
	let newSecret = $state('');
	let rotateProblem = $state('');
	/** The source with something shown under its row, and what. */
	let opened = $state<{ id: string; panel: 'setup' | 'requests' | 'mapping'; requests: SourceRequest[] } | null>(null);

	const byKind = $derived(Object.fromEntries(kinds.map((k) => [k.kind, k])));
	const chosenKind = $derived(byKind[kind]);
	const openedSource = $derived(sources.find((source) => source.id === opened?.id));

	const kindName = (source: Source) => byKind[source.kind]?.name ?? source.kind;

	const columns: Column<Source>[] = [
		{ key: 'name', label: 'Name', sort: (s) => s.name },
		{ key: 'kind', label: 'Kind', sort: kindName, from: 'md' },
		{ key: 'key', label: 'Key prefix', sort: (s) => (isBuiltIn(s) ? null : s.key_prefix), from: 'lg' },
		{ key: 'created', label: 'Created', sort: (s) => s.created_at, descendingFirst: true, from: 'md' },
		{ key: 'actions', label: 'Actions', end: true }
	];

	async function load() {
		sources = await api<Source[]>('GET', '/api/sources');
		loaded = true;
	}

	$effect(() => {
		Promise.all([load(), api<SourceKind[]>('GET', '/api/source-kinds').then((list) => (kinds = list))]).catch(
			(error) => toasts.problem(error)
		);
	});

	/** The address a sender posts to. */
	function addressOf(source: Source): string {
		return page.url.origin + source.path;
	}

	function setupOf(source: Source, key?: string): string {
		return setupText(byKind[source.kind]?.setup ?? '', { origin: page.url.origin, path: source.path, key });
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		problem = '';
		try {
			const created = await api<SourceCreated>('POST', '/api/sources', {
				name,
				kind,
				...(chosenKind?.own_secret ? { key: ownSecret } : {})
			});
			revealed = { source: created, key: created.key };
			name = '';
			ownSecret = '';
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	function startRotate(source: Source) {
		newSecret = '';
		rotateProblem = '';
		rotating = source;
	}

	async function rotate() {
		if (!rotating) return;
		const own = byKind[rotating.kind]?.own_secret ?? false;
		if (own && newSecret.trim() === '') {
			rotateProblem = 'Enter the new secret from the sender.';
			return;
		}
		try {
			const rotated = await api<{ key: string }>(
				'POST',
				`/api/sources/${rotating.id}/rotate`,
				newSecret.trim() === '' ? undefined : { key: newSecret.trim() }
			);
			revealed = { source: rotating, key: rotated.key };
			rotating = null;
			await load();
		} catch (error) {
			rotateProblem = messageOf(error);
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/sources/${deleting.id}`);
			toasts.ok('Source deleted.');
			if (opened?.id === deleting.id) opened = null;
			if (revealed?.source.id === deleting.id) revealed = null;
			deleting = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	/** Shows one thing under a source's row, or hides it when it is already shown. */
	async function toggle(source: Source, panel: 'setup' | 'requests' | 'mapping') {
		if (opened?.id === source.id && opened.panel === panel) {
			opened = null;
			return;
		}
		try {
			const requests =
				panel === 'setup' ? [] : await api<SourceRequest[]>('GET', `/api/sources/${source.id}/requests`);
			opened = { id: source.id, panel, requests };
			reveal('source-panel');
		} catch (error) {
			toasts.problem(error);
		}
	}

	function isOpen(source: Source, panel: 'setup' | 'requests' | 'mapping'): boolean {
		return opened?.id === source.id && opened.panel === panel;
	}

	const requestsWord = (source: Source) => (source.kind === NATIVE ? 'Rejected requests' : 'Recent requests');
</script>

<PageHeader title="Sources">
	A source is a service that sends events: one of your own, or a tool such as Alertmanager or GitHub. Each has
	its own key, so you can tell senders apart and cut one off without touching the others.
</PageHeader>

{#if revealed}
	{@const own = byKind[revealed.source.kind]?.own_secret ?? false}
	<div class="card border-gold mb-5 max-w-4xl">
		<div class="card-head"><h2>{revealed.source.name} is ready to receive</h2></div>
		<div class="card-body flex flex-col gap-3">
			{#if own}
				<p>ramzes checks each request against the secret you entered.</p>
			{:else}
				<p>Copy the key now. It cannot be shown again.</p>
				<CopyBlock text={revealed.key} testid="source-key" />
			{/if}
			{#if revealed.source.kind !== NATIVE}
				<p class="text-muted">The address to give the sender:</p>
				<CopyBlock text={addressOf(revealed.source)} testid="source-address" />
			{/if}
			<p class="text-muted">
				{revealed.source.kind === NATIVE ? 'Send a first event with it:' : 'How to set it up:'}
			</p>
			<CopyBlock text={setupOf(revealed.source, own ? undefined : revealed.key)} testid="source-setup" />
			<div>
				<button type="button" class="btn" onclick={() => (revealed = null)}>
					{own ? 'Done' : 'I have saved the key'}
				</button>
			</div>
		</div>
	</div>
{/if}

<DataTable
	rows={sources}
	{columns}
	key={(source) => source.id}
	noun="sources"
	singular="source"
	search={(s) => `${s.name} ${kindName(s)} ${s.kind} ${isBuiltIn(s) ? '' : s.key_prefix}`}
	initialSort={{ key: 'name', descending: false }}
	{loaded}
>
	{#snippet cell(source, column)}
		{#if column.key === 'name'}
			<div class="min-w-28">
				<div class="font-medium break-words">{source.name}</div>
				<!-- The kind has a column of its own from a tablet up. -->
				<div class="text-muted md:hidden">{kindName(source)}</div>
				{#if isBuiltIn(source)}
					<p class="hint max-w-md">
						Where ramzes's own events come from, such as a connection being paused or a message that could not be
						delivered. Nothing outside sends to it. Route its events with the topic <code>ramzes.*</code>
					</p>
				{/if}
			</div>
		{:else if column.key === 'kind'}
			<span class="tag">{kindName(source)}</span>
		{:else if column.key === 'key'}
			{#if !isBuiltIn(source)}<span class="mono text-muted whitespace-nowrap">{source.key_prefix}…</span>{/if}
		{:else if column.key === 'created'}
			<time class="text-muted whitespace-nowrap">{ago(source.created_at)}</time>
		{:else if !isBuiltIn(source)}
			<div class="flex flex-wrap justify-end gap-1 xl:flex-nowrap">
				<button type="button" class="btn btn-sm" aria-expanded={isOpen(source, 'setup')} onclick={() => toggle(source, 'setup')}>
					Setup
				</button>
				<button
					type="button"
					class="btn btn-sm"
					aria-expanded={isOpen(source, 'requests')}
					onclick={() => toggle(source, 'requests')}
				>
					{requestsWord(source)}
				</button>
				{#if source.kind === JSON_KIND}
					<button
						type="button"
						class="btn btn-sm"
						aria-expanded={isOpen(source, 'mapping')}
						onclick={() => toggle(source, 'mapping')}
					>
						Mapping
					</button>
				{/if}
				<button type="button" class="btn btn-sm" onclick={() => startRotate(source)}>Replace key</button>
				<button type="button" class="btn btn-sm btn-danger" onclick={() => (deleting = source)}>Delete</button>
			</div>
		{/if}
	{/snippet}
	{#snippet empty()}
		No sources yet. Add one below to get an address and a key a service can send events with.
	{/snippet}
</DataTable>

{#if opened && openedSource}
	{@const source = openedSource}
	<section id="source-panel" class="card mt-5 scroll-mt-16 {opened.panel === 'mapping' ? '' : 'max-w-4xl'}">
		<div class="card-head">
			<h2 class="mr-auto">
				{opened.panel === 'setup' ? 'Setup' : opened.panel === 'mapping' ? 'Mapping' : requestsWord(source)}:
				{source.name}
			</h2>
			<button type="button" class="btn btn-sm btn-quiet btn-icon" aria-label="Close" onclick={() => (opened = null)}>
				<Icon name="close" />
			</button>
		</div>
		<div class="card-body">
			{#if opened.panel === 'setup'}
				<div class="flex flex-col gap-2">
					{#if source.kind !== NATIVE}
						<p class="text-muted">The address to give the sender:</p>
						<CopyBlock text={addressOf(source)} />
					{/if}
					<CopyBlock text={setupOf(source)} />
					<p class="hint">
						The key is shown only when it is made. If it is lost, replace it and use the new one.
					</p>
				</div>
			{:else if opened.panel === 'requests'}
				<div class="requests flex flex-col">
					{#each opened.requests as request (request.id)}
						{@const headers = headerList(request.headers)}
						<div>
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
								<time class="text-muted">{clock(request.received_at)}</time>
								{#if request.error}
									<span class="badge" style="--tone: var(--st-failed)">rejected</span>
									<span class="problem min-w-0 break-words">{request.error}</span>
								{:else}
									<span class="badge" style="--tone: var(--st-delivered)">accepted</span>
								{/if}
							</div>
							{#if headers.length > 0}
								<details class="mt-2">
									<summary class="text-muted cursor-pointer">
										{headers.length} {headers.length === 1 ? 'header' : 'headers'}
									</summary>
									<div class="code mt-1">
										{#each headers as [header, value] (header)}
											<div><span class="text-muted">{header}:</span> {value}</div>
										{/each}
									</div>
								</details>
							{/if}
							<pre class="code mt-2 max-h-40 overflow-auto">{request.body}</pre>
						</div>
					{:else}
						<p class="hint">
							{source.kind === NATIVE
								? `ramzes has not rejected any request from ${source.name}.`
								: `No request from ${source.name} has arrived yet.`}
						</p>
					{/each}
				</div>
			{:else}
				{#key source.id}
					<MappingEditor {source} requests={opened.requests} onsaved={() => load().catch((error) => toasts.problem(error))} />
				{/key}
			{/if}
		</div>
	</section>
{/if}

<form class="card mt-5 max-w-xl" onsubmit={create}>
	<div class="card-head"><h2>Add a source</h2></div>
	<div class="card-body flex flex-col gap-4">
		<div>
			<label class="field-label" for="source-kind">What sends the events</label>
			<select id="source-kind" class="input" bind:value={kind}>
				{#each kinds as option (option.kind)}<option value={option.kind}>{option.name}</option>{/each}
			</select>
			{#if chosenKind}<p class="hint mt-1">{chosenKind.description}</p>{/if}
		</div>
		<div>
			<label class="field-label" for="source-name">Name</label>
			<input id="source-name" class="input" required maxlength="100" placeholder="orders-api" bind:value={name} />
		</div>
		{#if chosenKind?.own_secret}
			<div>
				<label class="field-label" for="source-secret">Secret from {chosenKind.name}</label>
				<input id="source-secret" class="input" type="password" autocomplete="off" required bind:value={ownSecret} />
				<p class="hint mt-1">
					{chosenKind.name} generates this secret and signs each request with it. Paste it here so ramzes can
					check the signature.
				</p>
			</div>
		{/if}
		{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
		<div><button type="submit" class="btn btn-primary" disabled={busy}>Create source</button></div>
	</div>
</form>

<Confirm
	open={rotating !== null}
	title="Replace the key of {rotating?.name ?? 'this source'}?"
	action="Replace key"
	onconfirm={rotate}
	oncancel={() => (rotating = null)}
>
	The current key stops working immediately. The service will be rejected until it uses the new one.
	{#if rotating && byKind[rotating.kind]?.own_secret}
		<label class="field-label mt-3" for="new-secret">New secret from {byKind[rotating.kind].name}</label>
		<input id="new-secret" class="input text-fg" type="password" autocomplete="off" bind:value={newSecret} />
	{:else if rotating}
		<label class="field-label mt-3" for="new-secret">Secret from the sender, if it generates its own</label>
		<input id="new-secret" class="input text-fg" type="password" autocomplete="off" bind:value={newSecret} />
		<span class="hint mt-1 block">Leave empty and ramzes generates a key.</span>
	{/if}
	{#if rotateProblem}<span class="problem mt-2 block" role="alert">{rotateProblem}</span>{/if}
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

<style>
	.requests > div + div {
		margin-top: 1rem;
		padding-top: 1rem;
		border-top: 1px solid var(--line);
	}
</style>
