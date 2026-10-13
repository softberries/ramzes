<script lang="ts">
	import { resolve } from '$app/paths';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ConnectorDescriptor, EventPage, EventRow, Template, TemplatePreview } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import RequestBlock from '#lib/components/RequestBlock.svelte';
	import { debounced } from '#lib/debounce.ts';
	import { clock } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { reveal } from '#lib/reveal.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** How long typing must pause before the preview is asked for again. */
	const PREVIEW_DELAY_MS = 300;

	/** The template being created or edited. `id` is null for a new one. */
	interface Draft {
		id: string | null;
		name: string;
		title: string;
		body: string;
	}

	let templates = $state<Template[]>([]);
	let events = $state<EventRow[]>([]);
	let loaded = $state(false);
	let draft = $state<Draft | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let deleting = $state<Template | null>(null);
	/** Why the server refused to delete a template, shown in the confirmation. */
	let deleteProblem = $state('');

	/** The event and connection the preview is made with. Empty means a sample event, no connection. */
	let eventId = $state('');
	let connectionId = $state('');
	let destination = $state('');
	let connectors = $state<ConnectorDescriptor[]>([]);

	/** What the chosen connection calls a destination, or null when its connector takes none. */
	const destinationLabel = $derived.by(() => {
		const kind = lookups.connections.find((c) => c.id === connectionId)?.kind;
		return connectors.find((c) => c.kind === kind)?.destination_label ?? null;
	});
	let preview = $state<TemplatePreview | null>(null);
	let previewProblem = $state('');

	async function load() {
		templates = await api<Template[]>('GET', '/api/templates');
		loaded = true;
	}

	$effect(() => {
		Promise.all([
			load(),
			lookups.load(),
			api<EventPage>('GET', '/api/events?limit=20').then((page) => (events = page.events)),
			api<ConnectorDescriptor[]>('GET', '/api/connectors').then((list) => (connectors = list))
		]).catch((error) => toasts.problem(error));
	});

	/** Counts preview requests, so a slow answer never replaces a newer one. */
	let asked = 0;

	async function askPreview() {
		if (!draft) return;
		const mine = ++asked;
		try {
			const answer = await api<TemplatePreview>('POST', '/api/templates/preview', {
				title: draft.title,
				body: draft.body,
				...(eventId ? { event_id: eventId } : {}),
				...(connectionId ? { connection_id: connectionId } : {}),
				...(destinationLabel && destination.trim() ? { destination: destination.trim() } : {})
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
		void [draft?.id, draft?.title, draft?.body, eventId, connectionId, destinationLabel, destination];
		if (draft) askSoon();
		return askSoon.cancel;
	});

	function open(next: Draft) {
		problem = '';
		preview = null;
		previewProblem = '';
		draft = next;
		reveal('template-form');
	}

	function startNew() {
		open({ id: null, name: '', title: '[{{ severity }}] {{ title }}', body: '{{ body }}' });
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!draft) return;
		busy = true;
		problem = '';
		const body = { name: draft.name, title: draft.title, body: draft.body };
		try {
			if (draft.id) await api('PUT', `/api/templates/${draft.id}`, body);
			else await api('POST', '/api/templates', body);
			toasts.ok(draft.id ? 'Template saved.' : 'Template created.');
			draft = null;
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/templates/${deleting.id}`);
			toasts.ok('Template deleted.');
			if (draft?.id === deleting.id) draft = null;
			deleting = null;
			await load();
		} catch (error) {
			deleteProblem = messageOf(error);
		}
	}

	const REFERENCE: [string, string][] = [
		['title, body', 'The text the event was sent with.'],
		['topic, severity, status', 'For example orders.failed, error, firing.'],
		['labels.<key>', 'One label, as in labels.env.'],
		['payload.<field>', 'A field of the JSON payload, as in payload.order.id.'],
		['count, events', 'How many events the message covers, and the list of them.'],
		['| cut(n)', 'Shortens text to n characters.'],
		['| default(...)', 'Used when the value is missing.'],
		['| tojson', 'Writes a value as JSON.']
	];
	const EXAMPLE = "[{{ labels.env | default('dev') }}] {{ title | cut(80) }}";

	const columns: Column<Template>[] = [
		{ key: 'name', label: 'Name', sort: (t) => t.name },
		{ key: 'title', label: 'Title', sort: (t) => t.title, from: 'md' },
		{ key: 'body', label: 'Body', sort: (t) => t.body || null, from: 'lg' },
		{ key: 'actions', label: 'Actions', end: true }
	];
</script>

<PageHeader title="Templates">
	A template words the message for an event. Choose one per connection on a route under
	<a href={resolve('/routes')}>Routes</a>; without one the event's own title and body are sent.
	{#snippet actions()}
		<button type="button" class="btn btn-primary" onclick={startNew}>
			<Icon name="plus" />
			New template
		</button>
	{/snippet}
</PageHeader>

<DataTable
	rows={templates}
	{columns}
	key={(template) => template.id}
	noun="templates"
	singular="template"
	search={(t) => `${t.name} ${t.title} ${t.body}`}
	initialSort={{ key: 'name', descending: false }}
	{loaded}
>
	{#snippet cell(template, column)}
		{#if column.key === 'name'}
			<div class="font-medium break-words">{template.name}</div>
			<!-- The title has a column of its own from a tablet up. -->
			<div class="mono text-muted break-all md:hidden">{template.title}</div>
		{:else if column.key === 'title'}
			<span class="mono text-muted block max-w-md truncate" title={template.title}>{template.title}</span>
		{:else if column.key === 'body'}
			{#if template.body}
				<span class="mono text-muted block max-w-sm truncate" title={template.body}>{template.body}</span>
			{:else}
				<span class="text-faint">the title alone</span>
			{/if}
		{:else}
			<div class="flex flex-wrap justify-end gap-1">
				<button type="button" class="btn btn-sm" onclick={() => open({ ...template })}>Edit</button>
				<button
					type="button"
					class="btn btn-sm btn-danger"
					onclick={() => {
						deleteProblem = '';
						deleting = template;
					}}>Delete</button
				>
			</div>
		{/if}
	{/snippet}
	{#snippet empty()}
		No templates yet, so every message uses the event's own title and body.
		<button type="button" class="text-accent underline" onclick={startNew}>Create the first template.</button>
	{/snippet}
</DataTable>

{#if draft}
	<div id="template-form" class="mt-5 grid scroll-mt-16 items-start gap-5 xl:grid-cols-2">
		<form class="card min-w-0" onsubmit={save}>
			<div class="card-head"><h2>{draft.id ? 'Edit template' : 'New template'}</h2></div>
			<div class="card-body flex flex-col gap-4">
				<div class="max-w-md">
					<label class="field-label" for="template-name">Name</label>
					<input id="template-name" class="input" required maxlength="100" bind:value={draft.name} />
				</div>
				<div>
					<label class="field-label" for="template-title">Title</label>
					<input
						id="template-title"
						class="input mono"
						required
						autocomplete="off"
						spellcheck="false"
						bind:value={draft.title}
					/>
				</div>
				<div>
					<label class="field-label" for="template-body">Body</label>
					<textarea
						id="template-body"
						class="input h-40 py-2"
						spellcheck="false"
						bind:value={draft.body}
					></textarea>
					<p class="hint mt-1">Leave empty to send the title alone.</p>
				</div>
				{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
				<div class="flex gap-2">
					<button type="submit" class="btn btn-primary" disabled={busy}>
						{draft.id ? 'Save template' : 'Create template'}
					</button>
					<button type="button" class="btn" onclick={() => (draft = null)}>Cancel</button>
				</div>
			</div>

			<details class="border-line border-t px-[1.1rem] py-3.5">
				<summary class="cursor-pointer font-medium">What a template can use</summary>
				<dl class="mt-3 grid gap-x-6 gap-y-1 md:grid-cols-[auto_1fr]">
					{#each REFERENCE as [name, meaning] (name)}
						<dt class="mono whitespace-nowrap">{name}</dt>
						<dd class="text-muted mb-1 md:mb-0">{meaning}</dd>
					{/each}
				</dl>
				<p class="hint mt-3">Values go between double braces. For example:</p>
				<pre class="code mt-1">{EXAMPLE}</pre>
			</details>
		</form>

		<section class="card min-w-0" aria-label="Preview">
			<div class="card-head"><h2>Preview</h2></div>
			<div class="card-body flex flex-col gap-4">
				<div class="grid gap-4 md:grid-cols-2">
					<div>
						<label class="field-label" for="preview-event">With the event</label>
						<select id="preview-event" class="input" bind:value={eventId}>
							<option value="">A sample event</option>
							{#each events as event (event.id)}
								<option value={event.id}>{clock(event.received_at)} {event.title}</option>
							{/each}
						</select>
					</div>
					<div>
						<label class="field-label" for="preview-connection">As sent to</label>
						<select id="preview-connection" class="input" bind:value={connectionId}>
							<option value="">No connection</option>
							{#each lookups.connections as connection (connection.id)}
								<option value={connection.id}>{connection.name}</option>
							{/each}
						</select>
					</div>
					{#if destinationLabel}
						<div class="md:col-start-2">
							<label class="field-label" for="preview-destination">{destinationLabel}</label>
							<input
								id="preview-destination"
								class="input"
								placeholder="The connection's own"
								autocomplete="off"
								bind:value={destination}
							/>
						</div>
					{/if}
				</div>

				{#if previewProblem}
					<p class="problem" role="alert">{previewProblem}</p>
				{:else if preview}
					{#if preview.error}
						<p class="problem" role="alert">
							{preview.error}
							<span class="text-muted block">Until this is fixed, the event's own title and body are sent:</span>
						</p>
					{/if}
					<div class="message">
						<div class="font-medium break-words" data-testid="preview-title">{preview.title}</div>
						{#if preview.body}
							<p class="text-muted mt-1 break-words whitespace-pre-wrap" data-testid="preview-body">{preview.body}</p>
						{/if}
					</div>
					{#if preview.request}
						<div>
							<h3 class="field-label">The request this connection sends</h3>
							<RequestBlock request={preview.request} />
						</div>
					{:else if preview.request_error}
						<p class="problem">The request could not be built: {preview.request_error}</p>
					{:else if !connectionId}
						<p class="hint">Choose a connection to see the exact request it would send.</p>
					{/if}
				{/if}
			</div>
		</section>
	</div>
{/if}

<Confirm
	open={deleting !== null}
	title="Delete {deleting?.name ?? 'this template'}?"
	action="Delete template"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	{#if deleteProblem}
		<span class="problem" role="alert">{deleteProblem}</span>
	{:else}
		A template that a route still uses cannot be deleted.
	{/if}
</Confirm>

<style>
	/* The message as its reader gets it: set apart from the form around it like a quotation. */
	.message {
		padding: 0.75rem 0.9rem;
		border: 1px solid var(--line);
		border-left: 3px solid var(--accent);
		border-radius: var(--radius-control);
		background: var(--bg);
	}
</style>
