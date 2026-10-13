<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { DefinitionRow } from '#lib/api/types.ts';
	import { EXAMPLE_DEFINITION } from '#lib/connectors.ts';
	import { ago } from '#lib/format.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import Confirm from './Confirm.svelte';
	import Icon from './Icon.svelte';

	/** Called after a definition was added, changed or deleted, so the catalogue can be reloaded. */
	let { onchange }: { onchange: () => void } = $props();

	let definitions = $state<DefinitionRow[]>([]);
	/** The definition being written. `id` is null for a new one. */
	let draft = $state<{ id: string | null; text: string } | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let deleting = $state<DefinitionRow | null>(null);
	/** Why the server refused to delete a definition, shown in the confirmation. */
	let deleteProblem = $state('');

	async function load() {
		definitions = await api<DefinitionRow[]>('GET', '/api/connector-definitions');
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
	});

	function open(next: { id: string | null; text: string }) {
		problem = '';
		draft = next;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!draft) return;
		busy = true;
		problem = '';
		try {
			const body = { definition: draft.text };
			if (draft.id) await api('PUT', `/api/connector-definitions/${draft.id}`, body);
			else await api('POST', '/api/connector-definitions', body);
			toasts.ok(draft.id ? 'Connector saved.' : 'Connector added.');
			draft = null;
			await load();
			onchange();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/connector-definitions/${deleting.id}`);
			toasts.ok('Connector deleted.');
			if (draft?.id === deleting.id) draft = null;
			deleting = null;
			await load();
			onchange();
		} catch (error) {
			deleteProblem = messageOf(error);
		}
	}
</script>

<section class="card max-w-4xl overflow-hidden" aria-label="Your own connectors">
	<div class="card-head">
		<div class="mr-auto min-w-0">
			<h2>Your own connectors</h2>
			<p class="hint">
				For a service ramzes does not ship a connector for: describe the request to send as YAML, and it appears
				in the list above.
			</p>
		</div>
		{#if !draft}
			<button type="button" class="btn" onclick={() => open({ id: null, text: EXAMPLE_DEFINITION })}>
				<Icon name="plus" />
				Write a connector
			</button>
		{/if}
	</div>

	{#if definitions.length > 0}
		<div class="overflow-x-auto">
			<table class="table" aria-label="Your own connectors">
				<thead>
					<tr>
						<th scope="col">Name</th>
						<th scope="col">Kind</th>
						<th scope="col" class="hidden md:table-cell">Added</th>
						<th scope="col" class="num">Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each definitions as definition (definition.id)}
						<tr>
							<td class="font-medium break-words">{definition.name}</td>
							<td><span class="mono text-muted break-all">{definition.kind}</span></td>
							<td class="hidden md:table-cell">
								<time class="text-muted whitespace-nowrap">{ago(definition.created_at)}</time>
							</td>
							<td class="num">
								<div class="flex flex-wrap justify-end gap-1">
									<button
										type="button"
										class="btn btn-sm"
										onclick={() => open({ id: definition.id, text: definition.definition })}>Edit</button
									>
									<button
										type="button"
										class="btn btn-sm btn-danger"
										onclick={() => {
											deleteProblem = '';
											deleting = definition;
										}}>Delete</button
									>
								</div>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}

	{#if draft}
		<form class="card-body flex flex-col gap-3 {definitions.length > 0 ? 'border-line border-t' : ''}" onsubmit={save}>
			<label class="font-medium" for="definition-text">{draft.id ? 'Edit connector' : 'New connector'}</label>
			<textarea
				id="definition-text"
				class="input h-72 py-2 whitespace-pre"
				spellcheck="false"
				required
				bind:value={draft.text}
			></textarea>
			<p class="hint">
				The URL, headers and body are templates. They can use <code>settings.&lt;key&gt;</code> and
				<code>credentials.&lt;key&gt;</code> from the fields, <code>message.title</code>, <code>message.body</code>,
				<code>message.severity</code>, <code>message.link</code>, and <code>destination</code>.
			</p>
			{#if problem}<p class="problem break-words" role="alert">{problem}</p>{/if}
			<div class="flex gap-2">
				<button type="submit" class="btn btn-primary" disabled={busy}>
					{draft.id ? 'Save connector' : 'Add connector'}
				</button>
				<button type="button" class="btn" onclick={() => (draft = null)}>Cancel</button>
			</div>
		</form>
	{/if}
</section>

<Confirm
	open={deleting !== null}
	title="Delete the connector {deleting?.name ?? ''}?"
	action="Delete connector"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	{#if deleteProblem}
		<span class="problem" role="alert">{deleteProblem}</span>
	{:else}
		A connector that a connection still uses cannot be deleted.
	{/if}
</Confirm>
