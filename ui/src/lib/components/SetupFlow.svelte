<script lang="ts">
	import { resolve } from '$app/paths';
	import { api, messageOf } from '#lib/api/client.ts';
	import type {
		ConnectionView,
		ConnectorDescriptor,
		EventPage,
		Journey,
		SourceCreated,
		TestResult
	} from '#lib/api/types.ts';
	import { curlForEvent } from '#lib/snippets.ts';
	import { live } from '#lib/stream.svelte.ts';
	import ConnectionForm, { type ConnectionValues } from './ConnectionForm.svelte';
	import CopyBlock from './CopyBlock.svelte';
	import StateTag from './StateTag.svelte';

	interface Props {
		/** Address of this instance, for the sample command. */
		origin: string;
		/** Called once the admin account exists and the browser is signed in. */
		onadmin: () => void | Promise<void>;
		/** True when the admin already exists and setup resumes at the source step. */
		started: boolean;
	}

	let { origin, onadmin, started }: Props = $props();

	/** How often to look for the test event and its delivery, besides listening to the stream. */
	const POLL_MS = 1500;

	// svelte-ignore state_referenced_locally
	let hasAdmin = $state(started);
	let email = $state('');
	let password = $state('');
	let sourceName = $state('my-service');
	let source = $state<SourceCreated | null>(null);
	let eventId = $state<string | null>(null);
	let connectors = $state<ConnectorDescriptor[]>([]);
	let kind = $state('slack');
	let connection = $state<ConnectionView | null>(null);
	let testResult = $state<TestResult | null>(null);
	let routeCreated = false;
	let routed = $state(false);
	let journey = $state<Journey | null>(null);
	let busy = $state(false);
	/** The problem to show, and the step it belongs to. */
	let problem = $state<{ step: number; text: string } | null>(null);

	const descriptor = $derived(connectors.find((c) => c.kind === kind) ?? connectors[0]);
	const delivery = $derived(journey?.notifications[0]?.deliveries[0] ?? null);
	const settled = $derived(delivery !== null && !['pending', 'in_progress'].includes(delivery.state));

	/** Runs one step's request, showing its error under that step. */
	async function attempt(step: number, action: () => Promise<void>) {
		busy = true;
		problem = null;
		try {
			await action();
		} catch (error) {
			problem = { step, text: messageOf(error) };
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		if (hasAdmin && connectors.length === 0) {
			api<ConnectorDescriptor[]>('GET', '/api/connectors')
				.then((list) => {
					connectors = list;
					if (!list.some((c) => c.kind === kind) && list[0]) kind = list[0].kind;
				})
				.catch((error) => (problem = { step: 4, text: messageOf(error) }));
		}
	});

	// While waiting for the test event, look for it whenever the stream reports one and on a timer.
	$effect(() => {
		if (!source || eventId) return;
		const sourceId = source.id;
		const look = async () => {
			const page = await api<EventPage>('GET', `/api/events?source=${sourceId}&limit=1`).catch(() => null);
			if (page?.events[0]) eventId = page.events[0].id;
		};
		const off = live.on('event', look);
		const timer = setInterval(look, POLL_MS);
		return () => {
			off();
			clearInterval(timer);
		};
	});

	// After the route exists, follow the test event's delivery until it settles.
	$effect(() => {
		if (!routed || !eventId || settled) return;
		const id = eventId;
		const look = async () => {
			journey = await api<Journey>('GET', `/api/events/${id}`).catch(() => journey);
		};
		look();
		const off = live.on('delivery', look);
		const timer = setInterval(look, POLL_MS);
		return () => {
			off();
			clearInterval(timer);
		};
	});

	function createAdmin(event: SubmitEvent) {
		event.preventDefault();
		attempt(1, async () => {
			await api('POST', '/api/setup', { email, password });
			hasAdmin = true;
			await onadmin();
		});
	}

	function createSource(event: SubmitEvent) {
		event.preventDefault();
		attempt(2, async () => {
			source = await api<SourceCreated>('POST', '/api/sources', { name: sourceName });
		});
	}

	function saveConnection(values: ConnectionValues) {
		attempt(4, async () => {
			testResult = null;
			connection = await api<ConnectionView>('POST', '/api/connections', { kind, ...values });
		});
	}

	function sendTest() {
		attempt(4, async () => {
			if (connection) testResult = await api<TestResult>('POST', `/api/connections/${connection.id}/test`);
		});
	}

	function createRoute() {
		attempt(5, async () => {
			if (!connection || !eventId) return;
			// If delivering the test event fails and the button is pressed again, the route already exists.
			if (!routeCreated) {
				await api('POST', '/api/routes', {
					name: 'Everything',
					match: {},
					targets: [{ connection_id: connection.id }]
				});
				routeCreated = true;
			}
			await api('POST', `/api/events/${eventId}/replay`);
			routed = true;
		});
	}
</script>

{#snippet step(n: number, title: string, done: boolean, locked: boolean)}
	<div class="flex items-center gap-3">
		<span
			class="marker {done ? 'border-ok text-ok' : locked ? 'border-line text-faint' : 'border-gold text-gold'}"
			aria-hidden="true">{done ? '✓' : n}</span
		>
		<h2 class={locked ? 'text-faint' : ''}>{title}</h2>
	</div>
{/snippet}

{#snippet trouble(n: number)}
	{#if problem?.step === n}<p class="problem" role="alert">{problem.text}</p>{/if}
{/snippet}

<ol class="card steps">
	{#if !started}
		<li class="flex flex-col gap-3 p-5">
			{@render step(1, 'Create your account', hasAdmin, false)}
			{#if !hasAdmin}
				<form class="ml-9 flex max-w-md flex-col gap-4" onsubmit={createAdmin}>
					<div>
						<label class="field-label" for="setup-email">Email</label>
						<input id="setup-email" class="input" type="email" autocomplete="username" required bind:value={email} />
					</div>
					<div>
						<label class="field-label" for="setup-password">Password</label>
						<input
							id="setup-password"
							class="input"
							type="password"
							autocomplete="new-password"
							required
							bind:value={password}
						/>
						<p class="hint mt-1">At least 10 characters.</p>
					</div>
					{@render trouble(1)}
					<div><button type="submit" class="btn btn-primary" disabled={busy}>Create account</button></div>
				</form>
			{:else}
				<p class="hint ml-9">Signed in as {email}.</p>
			{/if}
		</li>
	{/if}

	<li class="flex flex-col gap-3 p-5">
		{@render step(2, 'Name a source', source !== null, !hasAdmin)}
		{#if !source}
			<form class="ml-9 flex max-w-md flex-col gap-4" onsubmit={createSource}>
				<div>
					<label class="field-label" for="setup-source">The service that will send events</label>
					<input id="setup-source" class="input" required maxlength="100" disabled={!hasAdmin} bind:value={sourceName} />
				</div>
				{@render trouble(2)}
				<div>
					<button type="submit" class="btn btn-primary" disabled={!hasAdmin || busy}>Create source</button>
				</div>
			</form>
		{:else}
			<p class="hint ml-9">{source.name} can now send events with its own key.</p>
		{/if}
	</li>

	<li class="flex flex-col gap-3 p-5">
		{@render step(3, 'Send a test event', eventId !== null, !source)}
		{#if source}
			<div class="ml-9 flex flex-col gap-3">
				<p>Run this in a terminal. The key is shown only now, so keep it somewhere safe.</p>
				<CopyBlock text={curlForEvent(origin, source.key)} testid="curl" />
				{#if eventId}
					<p class="text-ok">Event received.</p>
				{:else}
					<p class="text-gold flex items-center gap-2">
						<span class="bg-gold inline-block h-2 w-2 animate-pulse rounded-full" aria-hidden="true"></span>
						Waiting for your event
					</p>
				{/if}
			</div>
		{/if}
	</li>

	<li class="flex flex-col gap-3 p-5">
		{@render step(4, 'Connect a channel', connection !== null, !eventId)}
		{#if eventId && descriptor}
			<div class="ml-9 flex max-w-md flex-col gap-4">
				{#if !connection}
					<div>
						<label class="field-label" for="setup-kind">Where should messages go?</label>
						<select id="setup-kind" class="input" bind:value={kind}>
							{#each connectors as c (c.kind)}<option value={c.kind}>{c.name}</option>{/each}
						</select>
						<p class="hint mt-1">{descriptor.description}</p>
					</div>
					{#key kind}
						<ConnectionForm
							{descriptor}
							onsubmit={saveConnection}
							submitLabel="Save connection"
							{busy}
							problem={problem?.step === 4 ? problem.text : ''}
						/>
					{/key}
				{:else}
					<p>{connection.name} is saved.</p>
					<div class="flex flex-wrap items-center gap-3">
						<button type="button" class="btn" disabled={busy} onclick={sendTest}>Send test message</button>
						{#if testResult?.ok}
							<span class="text-ok">Test message delivered.</span>
						{:else if testResult}
							<span class="problem">{testResult.detail}</span>
						{/if}
					</div>
					{@render trouble(4)}
				{/if}
			</div>
		{/if}
	</li>

	<li class="flex flex-col gap-3 p-5">
		{@render step(5, 'Route events to it', settled && delivery?.state === 'delivered', !connection)}
		{#if connection}
			<div class="ml-9 flex max-w-xl flex-col gap-3">
				{#if !routed}
					<p>
						This creates a route that sends every event to {connection.name}, then delivers your test event
						through it. You can narrow the route later.
					</p>
					{@render trouble(5)}
					<div>
						<button type="button" class="btn btn-primary" disabled={busy} onclick={createRoute}>
							Create route and deliver the test event
						</button>
					</div>
				{:else if !delivery || !settled}
					<p class="text-gold flex items-center gap-2">
						<span class="bg-gold inline-block h-2 w-2 animate-pulse rounded-full" aria-hidden="true"></span>
						Delivering your test event
					</p>
				{:else}
					<p>
						Your test event: <StateTag state={delivery.state} />
						{#if delivery.last_error}<span class="problem block">{delivery.last_error}</span>{/if}
					</p>
					<div class="flex flex-wrap gap-2">
						<a class="btn btn-primary no-underline" href={resolve('/')}>Open the feed</a>
						{#if eventId}
							<a class="btn no-underline" href={resolve('/events/[id]', { id: eventId })}>See what happened to it</a>
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	</li>
</ol>

<style>
	.steps > li + li {
		border-top: 1px solid var(--line);
	}

	.marker {
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1.5rem;
		height: 1.5rem;
		border-width: 1.5px;
		border-radius: 50%;
		font-size: 0.86rem;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
	}
</style>
