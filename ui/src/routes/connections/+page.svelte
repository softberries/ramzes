<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ConnectionView, ConnectorDescriptor, Route, TestResult } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import ConnectionForm, { type ConnectionValues } from '#lib/components/ConnectionForm.svelte';
	import ConnectorDefinitions from '#lib/components/ConnectorDefinitions.svelte';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import SlackSetup from '#lib/components/SlackSetup.svelte';
	import { capabilityWords, groupConnectors } from '#lib/connectors.ts';
	import { ago, timeOfDay } from '#lib/format.ts';
	import { proportion, shareWords } from '#lib/overview.ts';
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
	/**
	 * The Slack app connection whose setup is shown. One that was just saved is `fresh`: Slack still
	 * has to be told where to send button presses.
	 */
	let slack = $state<{ connection: ConnectionView; fresh: boolean } | null>(null);
	/** The latest test result per connection id. */
	let tests = $state<Record<string, TestResult>>({});
	let now = $state(Date.now());

	const byKind = $derived(Object.fromEntries(connectors.map((c) => [c.kind, c])));
	const groups = $derived(groupConnectors(connectors));
	const dependents = $derived(deleting ? routesUsing(routes, deleting.id) : []);

	const kindName = (connection: ConnectionView) => byKind[connection.kind]?.name ?? connection.kind;

	const columns: Column<ConnectionView>[] = [
		{ key: 'name', label: 'Name', sort: (c) => c.name },
		{ key: 'kind', label: 'Kind', sort: kindName, from: 'md' },
		{ key: 'delivered', label: 'Delivered, 24 h', sort: (c) => c.health?.delivered ?? 0, descendingFirst: true, end: true, from: 'md' },
		{ key: 'failed', label: 'Failed, 24 h', sort: (c) => c.health?.failed ?? 0, descendingFirst: true, end: true, from: 'md' },
		// Paused connections first: they are the ones that need someone.
		{ key: 'state', label: 'State', sort: (c) => (pausedUntil(c) ? 0 : 1) },
		{ key: 'changed', label: 'Changed', sort: (c) => c.updated_at, descendingFirst: true, from: 'lg' },
		{ key: 'actions', label: 'Actions', end: true }
	];

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
			slack = saved.kind === 'slack_app' ? { connection: saved, fresh: true } : null;
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
			if (slack?.connection.id === deleting.id) slack = null;
			deleting = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	function edit(connection: ConnectionView) {
		close();
		editing = connection;
		reveal('connection-form');
	}

	function add(kind: string) {
		close();
		adding = kind;
		reveal('connection-form');
	}
</script>

<PageHeader title="Connections">
	A connection is one place messages can go: a Slack channel, a mailbox, a webhook URL. Routes decide which
	events go to which connection.
</PageHeader>

{#if slack}
	<div id="slack-setup" class="card mb-5 max-w-3xl scroll-mt-16 {slack.fresh ? 'border-gold' : ''}" data-testid="slack-setup">
		<div class="card-head">
			<h2>{slack.fresh ? 'One more step in Slack for' : 'Slack setup for'} {slack.connection.name}</h2>
		</div>
		<div class="card-body flex flex-col gap-3">
			{#key slack.connection.id}<SlackSetup connectionId={slack.connection.id} />{/key}
			<div><button type="button" class="btn" onclick={() => (slack = null)}>Done</button></div>
		</div>
	</div>
{/if}

<DataTable
	rows={connections}
	{columns}
	key={(connection) => connection.id}
	noun="connections"
	singular="connection"
	search={(c) => `${c.name} ${kindName(c)} ${c.kind} ${c.health?.last_error ?? ''}`}
	initialSort={{ key: 'name', descending: false }}
	{loaded}
>
	{#snippet cell(connection, column)}
		{@const health = connection.health}
		{@const until = pausedUntil(connection)}
		{#if column.key === 'name'}
			<div class="md:min-w-32">
				<div class="font-medium break-words">{connection.name}</div>
				<!-- The kind has a column of its own from a tablet up. -->
				<div class="text-muted md:hidden">{kindName(connection)}</div>
				{#if health?.last_error}
					<div class="problem max-w-md break-words">Last error: {health.last_error}</div>
				{/if}
				{#if tests[connection.id]?.ok}
					<div class="text-ok">Test message delivered.</div>
				{:else if tests[connection.id]}
					<div class="problem max-w-md break-words">Test failed: {tests[connection.id].detail}</div>
				{/if}
			</div>
		{:else if column.key === 'kind'}
			<span class="tag">{kindName(connection)}</span>
		{:else if column.key === 'delivered'}
			{@const settled = (health?.delivered ?? 0) + (health?.failed ?? 0)}
			<div>{health?.delivered ?? 0}</div>
			{#if health && settled > 0}
				<div
					class="rate"
					role="img"
					aria-label="{shareWords(health.delivered, settled)} of {settled} arrived"
					title="{shareWords(health.delivered, settled)} arrived"
				>
					<div style="width: {proportion(health.delivered, settled)}%"></div>
				</div>
			{/if}
		{:else if column.key === 'failed'}
			<div class={health && health.failed > 0 ? 'problem' : 'text-muted'}>{health?.failed ?? 0}</div>
			{#if health && health.pending > 0}<div class="text-muted whitespace-nowrap">{health.pending} waiting</div>{/if}
		{:else if column.key === 'state'}
			{#if until}
				<div class="flex max-w-64 flex-wrap items-center gap-x-2 gap-y-1.5" data-testid="paused">
					<span class="badge" style="--tone: var(--st-failed)">Paused until {timeOfDay(until, now)}</span>
					<span class="text-muted">
						after {connection.failure_streak} failed
						{connection.failure_streak === 1 ? 'attempt' : 'attempts'} in a row
					</span>
					<button type="button" class="btn btn-sm" onclick={() => resume(connection)}>Resume now</button>
				</div>
			{:else}
				<span class="badge" style="--tone: var(--st-delivered)">Active</span>
			{/if}
		{:else if column.key === 'changed'}
			<time class="text-muted whitespace-nowrap">{ago(connection.updated_at, now)}</time>
		{:else}
			<div class="flex flex-wrap justify-end gap-1 xl:flex-nowrap">
				<button type="button" class="btn btn-sm" onclick={() => test(connection)}>
					Send test <span class="max-md:sr-only">message</span>
				</button>
				{#if connection.kind === 'slack_app'}
					<button
						type="button"
						class="btn btn-sm"
						aria-expanded={slack?.connection.id === connection.id}
						onclick={() => {
							slack = slack?.connection.id === connection.id ? null : { connection, fresh: false };
							reveal('slack-setup');
						}}>Setup</button
					>
				{/if}
				<button type="button" class="btn btn-sm" onclick={() => edit(connection)}>Edit</button>
				<button type="button" class="btn btn-sm btn-danger" onclick={() => (deleting = connection)}>Delete</button>
			</div>
		{/if}
	{/snippet}
	{#snippet empty()}
		No connections yet. Add one below.
	{/snippet}
</DataTable>

{#if editing && byKind[editing.kind]}
	<div id="connection-form" class="card mt-5 max-w-xl scroll-mt-16">
		<div class="card-head"><h2>Edit {editing.name}</h2></div>
		<div class="card-body">
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
	</div>
{:else if adding && byKind[adding]}
	<div id="connection-form" class="card mt-5 max-w-xl scroll-mt-16">
		<div class="card-head">
			<div>
				<h2>New {byKind[adding].name} connection</h2>
				<p class="hint">{byKind[adding].description}</p>
			</div>
		</div>
		<div class="card-body">
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
	</div>
{:else}
	<h2 class="mt-8 mb-3">Add a connection</h2>
	<div class="flex flex-col gap-5">
		{#each groups as group (group.title)}
			<section aria-label={group.title}>
				<h3 class="text-muted mb-2 font-medium">{group.title}</h3>
				<ul class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
					{#each group.connectors as connector (connector.kind)}
						<li>
							<button type="button" class="card connector" onclick={() => add(connector.kind)}>
								<span class="flex w-full items-center gap-2 font-medium">
									<span class="mr-auto">{connector.name}</span>
									{#if connector.custom}<span class="tag font-normal">your own</span>{/if}
									<span class="add"><Icon name="plus" /></span>
								</span>
								<span class="text-muted block">{connector.description}</span>
								{#if capabilityWords(connector.capabilities)}
									<span class="text-muted mt-auto block pt-1" data-testid="capabilities">
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

<div class="mt-8">
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

<style>
	/* How much of what ended arrived: the track is what failed. */
	.rate {
		width: 4.5rem;
		height: 0.3rem;
		margin: 0.3rem 0 0 auto;
		overflow: hidden;
		border-radius: 999px;
		background: var(--fill-failed);
	}

	.rate > div {
		height: 100%;
		background: var(--fill-delivered);
	}

	.connector {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 0.25rem;
		width: 100%;
		height: 100%;
		padding: 0.9rem 1rem;
		text-align: left;
		cursor: pointer;
		transition:
			border-color 120ms,
			background-color 120ms;
	}

	.connector:hover {
		border-color: var(--faint);
		background: var(--raised);
	}

	.add {
		color: var(--faint);
	}

	.connector:hover .add {
		color: var(--accent);
	}
</style>
