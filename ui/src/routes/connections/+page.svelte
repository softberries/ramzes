<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ConnectionView, ConnectorDescriptor, Route, TestResult } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import ConnectionForm, { type ConnectionValues } from '#lib/components/ConnectionForm.svelte';
	import ConnectorDefinitions from '#lib/components/ConnectorDefinitions.svelte';
	import SlackSetup from '#lib/components/SlackSetup.svelte';
	import { capabilityWords, groupConnectors, healthWords } from '#lib/connectors.ts';
	import { ago, timeOfDay } from '#lib/format.ts';
	import { reveal } from '#lib/reveal.ts';
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
	/** A Slack app connection that was just saved: Slack still has to be told where to send button presses. */
	let justSaved = $state<ConnectionView | null>(null);
	/** The latest test result per connection id. */
	let tests = $state<Record<string, TestResult>>({});
	let now = $state(Date.now());

	const byKind = $derived(Object.fromEntries(connectors.map((c) => [c.kind, c])));
	const groups = $derived(groupConnectors(connectors));
	const dependents = $derived(deleting ? routesUsing(routes, deleting.id) : []);

	async function loadConnectors() {
		connectors = await api<ConnectorDescriptor[]>('GET', '/api/connectors');
	}

	async function load() {
		[connections, connectors, routes] = await Promise.all([
			api<ConnectionView[]>('GET', '/api/connections'),
			api<ConnectorDescriptor[]>('GET', '/api/connectors'),
			api<Route[]>('GET', '/api/routes')
		]);
		now = Date.now();
		loaded = true;
	}

	/** Until when a connection is paused, or null when it is not paused now. */
	function pausedUntil(connection: ConnectionView): number | null {
		const until = connection.paused_until;
		return until && until > now ? until : null;
	}

	async function resume(connection: ConnectionView) {
		try {
			await api('POST', `/api/connections/${connection.id}/resume`);
			toasts.ok(`${connection.name} resumed. Waiting deliveries are sent now.`);
			await load();
		} catch (error) {
			toasts.problem(error);
		}
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
			const saved = editing
				? await api<ConnectionView>('PUT', `/api/connections/${editing.id}`, values)
				: await api<ConnectionView>('POST', '/api/connections', { kind: adding, ...values });
			toasts.ok(editing ? 'Connection saved.' : 'Connection added.');
			justSaved = saved.kind === 'slack_app' ? saved : null;
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
	A connection is one place messages can go: a Slack channel, a mailbox, a webhook URL. Routes decide
	which events go to which connection.
</p>

{#if justSaved}
	<div class="panel border-gold mb-5 flex max-w-3xl flex-col gap-3 p-4" data-testid="slack-setup">
		<h2>One more step in Slack for {justSaved.name}</h2>
		<SlackSetup connectionId={justSaved.id} />
		<div><button type="button" class="btn" onclick={() => (justSaved = null)}>Done</button></div>
	</div>
{/if}

{#if loaded && connections.length === 0}
	<p class="hint pb-4">No connections yet. Add one below.</p>
{/if}

<ul class="border-line border-t">
	{#each connections as connection (connection.id)}
		<li class="border-line grid items-baseline gap-x-3 gap-y-2 border-b px-2 py-2.5 md:grid-cols-[1fr_auto]">
			<div class="min-w-0">
				<div class="flex flex-wrap items-baseline gap-x-3">
					<span class="font-medium">{connection.name}</span>
					<span class="tag">{byKind[connection.kind]?.name ?? connection.kind}</span>
					<span class="text-faint">changed {ago(connection.updated_at)}</span>
				</div>
				{#if pausedUntil(connection)}
					<div class="problem flex flex-wrap items-baseline gap-x-3 gap-y-1" data-testid="paused">
						<span>
							Paused until {timeOfDay(pausedUntil(connection) ?? now, now)} after {connection.failure_streak} failed
							{connection.failure_streak === 1 ? 'attempt' : 'attempts'} in a row
						</span>
						<button type="button" class="btn h-7 px-2" onclick={() => resume(connection)}>Resume now</button>
					</div>
				{/if}
				{#if connection.health}
					<div class="text-muted">
						Last 24 hours: <span class={connection.health.failed > 0 ? 'problem' : ''}
							>{healthWords(connection.health)}</span
						>
					</div>
					{#if connection.health.last_error}
						<div class="problem break-words">Last error: {connection.health.last_error}</div>
					{/if}
				{/if}
				{#if tests[connection.id]?.ok}
					<div class="text-ok">Test message delivered.</div>
				{:else if tests[connection.id]}
					<div class="problem break-words">Test failed: {tests[connection.id].detail}</div>
				{/if}
				{#if connection.kind === 'slack_app'}
					<details class="mt-1">
						<summary class="text-muted cursor-pointer">Setup</summary>
						<div class="mt-2"><SlackSetup connectionId={connection.id} /></div>
					</details>
				{/if}
			</div>
			<div class="flex flex-wrap gap-1 md:justify-end">
				<button type="button" class="btn h-7 px-2" onclick={() => test(connection)}>Send test message</button>
				<button
					type="button"
					class="btn h-7 px-2"
					onclick={() => {
						close();
						editing = connection;
						reveal('connection-form');
					}}>Edit</button
				>
				<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (deleting = connection)}>Delete</button>
			</div>
		</li>
	{/each}
</ul>

{#if editing && byKind[editing.kind]}
	<div id="connection-form" class="panel mt-6 max-w-xl scroll-mt-16 p-4">
		<h2 class="mb-4">Edit {editing.name}</h2>
		{#key editing.id}
			<ConnectionForm
				descriptor={byKind[editing.kind]}
				initial={{ name: editing.name, settings: editing.settings as Record<string, unknown> }}
				onsubmit={save}
				submitLabel="Save connection"
				oncancel={close}
				{busy}
				{problem}
			/>
		{/key}
	</div>
{:else if adding && byKind[adding]}
	<div id="connection-form" class="panel mt-6 max-w-xl scroll-mt-16 p-4">
		<h2 class="mb-1">New {byKind[adding].name} connection</h2>
		<p class="hint mb-4">{byKind[adding].description}</p>
		{#key adding}
			<ConnectionForm
				descriptor={byKind[adding]}
				onsubmit={save}
				submitLabel="Add connection"
				oncancel={close}
				{busy}
				{problem}
			/>
		{/key}
	</div>
{:else}
	<h2 class="mt-8 mb-3">Add a connection</h2>
	<div class="flex max-w-3xl flex-col gap-4">
		{#each groups as group (group.title)}
			<section aria-label={group.title}>
				<h3 class="text-muted mb-2">{group.title}</h3>
				<ul class="grid gap-2 md:grid-cols-2">
					{#each group.connectors as connector (connector.kind)}
						<li>
							<button
								type="button"
								class="panel hover:border-muted flex h-full w-full flex-col items-start p-3 text-left"
								onclick={() => {
									close();
									adding = connector.kind;
									reveal('connection-form');
								}}
							>
								<span class="font-medium">
									{connector.name}
									{#if connector.custom}<span class="tag ml-2 font-normal">your own</span>{/if}
								</span>
								<span class="text-muted block">{connector.description}</span>
								{#if capabilityWords(connector.capabilities)}
									<span class="text-muted mt-1 block" data-testid="capabilities">
										<span class="text-faint">Can do:</span>
										{capabilityWords(connector.capabilities)}
									</span>
								{/if}
							</button>
						</li>
					{/each}
				</ul>
			</section>
		{/each}
	</div>
{/if}

<div class="mt-10">
	<ConnectorDefinitions onchange={() => loadConnectors().catch((error) => toasts.problem(error))} />
</div>

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
