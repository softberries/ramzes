<script lang="ts">
	import { page } from '$app/state';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { Source, SourceCreated, SourceKind, SourceRequest } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import CopyBlock from '#lib/components/CopyBlock.svelte';
	import MappingEditor from '#lib/components/MappingEditor.svelte';
	import { ago, clock } from '#lib/format.ts';
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
		} catch (error) {
			toasts.problem(error);
		}
	}

	function isOpen(source: Source, panel: 'setup' | 'requests' | 'mapping'): boolean {
		return opened?.id === source.id && opened.panel === panel;
	}
</script>

<h1 class="mb-2">Sources</h1>
<p class="hint mb-5 max-w-2xl">
	A source is a service that sends events: one of your own, or a tool such as Alertmanager or GitHub. Each
	has its own key, so you can tell senders apart and cut one off without touching the others.
</p>

{#if revealed}
	{@const own = byKind[revealed.source.kind]?.own_secret ?? false}
	<div class="panel border-gold mb-6 flex flex-col gap-3 p-4">
		<h2>{revealed.source.name} is ready to receive</h2>
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
{/if}

{#if loaded && sources.length === 0}
	<p class="hint pb-4">No sources yet. Add one below to get an address and a key a service can send events with.</p>
{/if}

<ul class="border-line border-t" aria-label="Sources">
	{#each sources as source (source.id)}
		<li class="border-line border-b px-2 py-2.5">
			<div class="grid items-baseline gap-x-3 gap-y-2 md:grid-cols-[1fr_auto]">
				<div class="flex min-w-0 flex-wrap items-baseline gap-x-3">
					<span class="font-medium">{source.name}</span>
					<span class="tag">{byKind[source.kind]?.name ?? source.kind}</span>
					{#if !isBuiltIn(source)}<span class="text-muted">{source.key_prefix}…</span>{/if}
					<span class="text-faint">created {ago(source.created_at)}</span>
				</div>
				{#if !isBuiltIn(source)}
					<div class="flex flex-wrap gap-1 md:justify-end">
						<button type="button" class="btn h-7 px-2" aria-expanded={isOpen(source, 'setup')} onclick={() => toggle(source, 'setup')}>
							Setup
						</button>
						<button
							type="button"
							class="btn h-7 px-2"
							aria-expanded={isOpen(source, 'requests')}
							onclick={() => toggle(source, 'requests')}
						>
							{source.kind === NATIVE ? 'Rejected requests' : 'Recent requests'}
						</button>
						{#if source.kind === JSON_KIND}
							<button
								type="button"
								class="btn h-7 px-2"
								aria-expanded={isOpen(source, 'mapping')}
								onclick={() => toggle(source, 'mapping')}
							>
								Mapping
							</button>
						{/if}
						<button type="button" class="btn h-7 px-2" onclick={() => startRotate(source)}>Replace key</button>
						<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (deleting = source)}>Delete</button>
					</div>
				{/if}
			</div>
			{#if isBuiltIn(source)}
				<p class="hint max-w-2xl">
					Where ramzes's own events come from, such as a connection being paused or a message that could not be
					delivered. Nothing outside sends to it. Route its events with the topic ramzes.*
				</p>
			{/if}

			{#if isOpen(source, 'setup')}
				<div class="mt-3 flex max-w-3xl flex-col gap-2">
					{#if source.kind !== NATIVE}
						<p class="text-muted">The address to give the sender:</p>
						<CopyBlock text={addressOf(source)} />
					{/if}
					<CopyBlock text={setupOf(source)} />
					<p class="hint">
						The key is shown only when it is made. If it is lost, replace it and use the new one.
					</p>
				</div>
			{:else if opened && isOpen(source, 'requests')}
				<div class="mt-3 flex flex-col gap-3">
					{#each opened.requests as request (request.id)}
						{@const headers = headerList(request.headers)}
						<div>
							<div class="flex flex-wrap gap-x-4">
								<time class="text-muted">{clock(request.received_at)}</time>
								{#if request.error}
									<span class="problem break-words">{request.error}</span>
								{:else}
									<span class="text-ok">accepted</span>
								{/if}
							</div>
							{#if headers.length > 0}
								<details class="mt-1">
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
							<pre class="code mt-1 max-h-40 overflow-auto">{request.body}</pre>
						</div>
					{:else}
						<p class="hint">
							{source.kind === NATIVE
								? `ramzes has not rejected any request from ${source.name}.`
								: `No request from ${source.name} has arrived yet.`}
						</p>
					{/each}
				</div>
			{:else if opened && isOpen(source, 'mapping')}
				<div class="mt-3">
					<MappingEditor {source} requests={opened.requests} onsaved={() => load().catch((error) => toasts.problem(error))} />
				</div>
			{/if}
		</li>
	{/each}
</ul>

<form class="mt-8 flex max-w-md flex-col gap-3" onsubmit={create}>
	<h2>Add a source</h2>
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
