<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ConnectionView, ConnectorDescriptor, Route, TestResult } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import ConnectionForm, { type ConnectionValues } from '#lib/components/ConnectionForm.svelte';
	import { ago } from '#lib/format.ts';
	import { routesUsing } from '#lib/routes.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let connections = $state<ConnectionView[]>([]);
	let connectors = $state<ConnectorDescriptor[]>([]);
	let routes = $state<Route[]>([]);
	let loaded = $state(false);
	/** The connector kind of a new connection, or the connection being edited. */
	let adding = $state<string | null>(null);
	let editing = $state<ConnectionView | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let deleting = $state<ConnectionView | null>(null);
	/** The latest test result per connection id. */
	let tests = $state<Record<string, TestResult>>({});

	const byKind = $derived(Object.fromEntries(connectors.map((c) => [c.kind, c])));
	const dependents = $derived(deleting ? routesUsing(routes, deleting.id) : []);

	async function load() {
		[connections, connectors, routes] = await Promise.all([
			api<ConnectionView[]>('GET', '/api/connections'),
			api<ConnectorDescriptor[]>('GET', '/api/connectors'),
			api<Route[]>('GET', '/api/routes')
		]);
		loaded = true;
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
	});

	function close() {
		adding = null;
		editing = null;
		problem = '';
	}

	async function save(values: ConnectionValues) {
		busy = true;
		problem = '';
		try {
			if (editing) await api('PUT', `/api/connections/${editing.id}`, values);
			else await api('POST', '/api/connections', { kind: adding, ...values });
			toasts.ok(editing ? 'Connection saved.' : 'Connection added.');
			close();
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function test(connection: ConnectionView) {
		try {
			tests[connection.id] = await api<TestResult>('POST', `/api/connections/${connection.id}/test`);
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/connections/${deleting.id}`);
			toasts.ok('Connection deleted.');
			deleting = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}
</script>

<div class="mb-2 flex items-center gap-3">
	<h1 class="mr-auto">Connections</h1>
</div>
<p class="hint mb-5 max-w-2xl">
	A connection is one place messages can go: a Slack channel, a webhook URL. Routes decide which events
	go to which connection.
</p>

{#if loaded && connections.length === 0}
	<p class="hint pb-4">No connections yet. Add one below.</p>
{/if}

<ul class="border-line border-t">
	{#each connections as connection (connection.id)}
		<li class="border-line grid grid-cols-[1fr_auto] items-baseline gap-x-3 border-b px-2 py-2.5">
			<div class="min-w-0">
				<div class="flex flex-wrap items-baseline gap-x-3">
					<span class="font-medium">{connection.name}</span>
					<span class="tag">{byKind[connection.kind]?.name ?? connection.kind}</span>
					<span class="text-faint">changed {ago(connection.updated_at)}</span>
				</div>
				{#if tests[connection.id]?.ok}
					<div class="text-ok">Test message delivered.</div>
				{:else if tests[connection.id]}
					<div class="problem">Test failed: {tests[connection.id].detail}</div>
				{/if}
			</div>
			<div class="flex flex-wrap justify-end gap-1">
				<button type="button" class="btn h-7 px-2" onclick={() => test(connection)}>Send test message</button>
				<button
					type="button"
					class="btn h-7 px-2"
					onclick={() => {
						close();
						editing = connection;
					}}>Edit</button
				>
				<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (deleting = connection)}>Delete</button>
			</div>
		</li>
	{/each}
</ul>

{#if editing && byKind[editing.kind]}
	<div class="panel mt-6 max-w-xl p-4">
		<h2 class="mb-4">Edit {editing.name}</h2>
		{#key editing.id}
			<ConnectionForm
				descriptor={byKind[editing.kind]}
				initial={{ name: editing.name, settings: editing.settings as Record<string, unknown> }}
				onsubmit={save}
				submitLabel="Save connection"
				{busy}
				{problem}
			/>
		{/key}
		<button type="button" class="btn btn-quiet mt-2" onclick={close}>Cancel</button>
	</div>
{:else if adding && byKind[adding]}
	<div class="panel mt-6 max-w-xl p-4">
		<h2 class="mb-1">New {byKind[adding].name} connection</h2>
		<p class="hint mb-4">{byKind[adding].description}</p>
		{#key adding}
			<ConnectionForm descriptor={byKind[adding]} onsubmit={save} submitLabel="Add connection" {busy} {problem} />
		{/key}
		<button type="button" class="btn btn-quiet mt-2" onclick={close}>Cancel</button>
	</div>
{:else}
	<h2 class="mt-8 mb-3">Add a connection</h2>
	<ul class="grid max-w-3xl gap-2 md:grid-cols-2">
		{#each connectors as connector (connector.kind)}
			<li>
				<button
					type="button"
					class="panel hover:border-muted w-full p-3 text-left"
					onclick={() => {
						close();
						adding = connector.kind;
					}}
				>
					<span class="font-medium">{connector.name}</span>
					<span class="text-muted block">{connector.description}</span>
				</button>
			</li>
		{/each}
	</ul>
{/if}

<Confirm
	open={deleting !== null}
	title="Delete {deleting?.name ?? 'this connection'}?"
	action="Delete connection"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	{#if dependents.length > 0}
		{dependents.length === 1 ? 'This route sends' : 'These routes send'} to it and will fail to deliver until
		changed:
		<ul class="text-fg mt-2 list-inside list-disc">
			{#each dependents as route (route.id)}<li>{route.name}</li>{/each}
		</ul>
	{:else}
		No route sends to it.
	{/if}
</Confirm>
