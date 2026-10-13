<script lang="ts">
	import { resolve } from '$app/paths';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ConnectorDescriptor, Route, RouteInput, RouteMatch, Template } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import EscalationEditor from '#lib/components/EscalationEditor.svelte';
	import MatchEditor from '#lib/components/MatchEditor.svelte';
	import RouteTester from '#lib/components/RouteTester.svelte';
	import ShapingEditor from '#lib/components/ShapingEditor.svelte';
	import TargetRows from '#lib/components/TargetRows.svelte';
	import {
		chainToForm,
		emptyTarget,
		formToChain,
		summarizeChain,
		targetRow,
		type StepForm,
		type TargetRow
	} from '#lib/escalation.ts';
	import { ago } from '#lib/format.ts';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { reveal } from '#lib/reveal.ts';
	import { cleanMatch, cleanTargets, inputOfRoute, moved, routesForTest, summarize } from '#lib/routes.ts';
	import { formToShaping, shapingToForm, summarizeShaping, type ShapingForm } from '#lib/shaping.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** The route being created or edited. `id` is null for a new one. */
	interface Draft {
		id: string | null;
		name: string;
		enabled: boolean;
		stop: boolean;
		match: RouteMatch;
		/** One row per target. Empty strings stand for "not chosen" and are cleaned before sending. */
		targets: TargetRow[];
		shaping: ShapingForm;
		/** Who else is told while nobody acknowledges, in order. */
		escalation: StepForm[];
	}

	/** Quiet hours start in the time zone of whoever sets them up. */
	const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

	let routes = $state<Route[]>([]);
	let templates = $state<Template[]>([]);
	let connectors = $state<ConnectorDescriptor[]>([]);
	let loaded = $state(false);
	let draft = $state<Draft | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let deleting = $state<Route | null>(null);

	const templateNames = $derived(Object.fromEntries(templates.map((t) => [t.id, t.name])));

	/** What to call the destination of a connection, or null when its connector takes none. */
	function destinationLabel(connectionId: string): string | null {
		const kind = lookups.connections.find((c) => c.id === connectionId)?.kind;
		return connectors.find((c) => c.kind === kind)?.destination_label ?? null;
	}

	/** The draft as the API takes it. */
	function inputOf(from: Draft): RouteInput {
		const takesDestination = (id: string) => destinationLabel(id) !== null;
		return {
			name: from.name,
			enabled: from.enabled,
			stop: from.stop,
			match: cleanMatch(from.match),
			targets: cleanTargets(from.targets, takesDestination),
			shaping: formToShaping(from.shaping),
			escalation: formToChain(from.escalation, takesDestination)
		};
	}

	/** The routes the tester tries: the stored ones with the draft in its place. */
	const tested = $derived(routesForTest(routes, draft && { id: draft.id, input: inputOf(draft) }));
	const draftIndex = $derived.by(() => {
		if (!draft) return null;
		const id = draft.id;
		return id === null ? tested.length - 1 : routes.findIndex((route) => route.id === id);
	});

	async function load() {
		routes = await api<Route[]>('GET', '/api/routes');
		loaded = true;
	}

	$effect(() => {
		Promise.all([
			load(),
			lookups.load(),
			api<Template[]>('GET', '/api/templates').then((list) => (templates = list)),
			api<ConnectorDescriptor[]>('GET', '/api/connectors').then((list) => (connectors = list))
		]).catch((error) => toasts.problem(error));
	});

	function startNew() {
		problem = '';
		draft = {
			id: null,
			name: '',
			enabled: true,
			stop: false,
			match: {},
			targets: [emptyTarget(lookups.connections[0]?.id)],
			shaping: shapingToForm(undefined, browserZone),
			escalation: []
		};
		reveal('route-form');
	}

	function startEdit(route: Route) {
		problem = '';
		draft = {
			id: route.id,
			name: route.name,
			enabled: route.enabled,
			stop: route.stop,
			match: structuredClone($state.snapshot(route.match)) as RouteMatch,
			targets: route.targets.map(targetRow),
			shaping: shapingToForm($state.snapshot(route.shaping), browserZone),
			escalation: chainToForm($state.snapshot(route.escalation))
		};
		reveal('route-form');
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!draft) return;
		busy = true;
		problem = '';
		const body = inputOf(draft);
		try {
			if (draft.id) await api('PUT', `/api/routes/${draft.id}`, body);
			else await api('POST', '/api/routes', body);
			toasts.ok(draft.id ? 'Route saved.' : 'Route created.');
			draft = null;
			await load();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}

	async function move(route: Route, step: 1 | -1) {
		const ids = moved(
			routes.map((r) => r.id),
			route.id,
			step
		);
		if (!ids) return;
		try {
			await api('POST', '/api/routes/reorder', { ids });
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function toggle(route: Route) {
		try {
			await api('PUT', `/api/routes/${route.id}`, { ...inputOfRoute(route), enabled: !route.enabled });
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function remove() {
		if (!deleting) return;
		try {
			await api('DELETE', `/api/routes/${deleting.id}`);
			toasts.ok('Route deleted.');
			if (draft?.id === deleting.id) draft = null;
			deleting = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		}
	}
</script>

<div class="mb-2 flex items-center gap-3">
	<h1 class="mr-auto">Routes</h1>
	<button type="button" class="btn btn-primary" onclick={startNew}>New route</button>
</div>
<p class="hint mb-5 max-w-2xl">
	Each event is checked against these routes from the top. Every route that matches sends it to its
	connections, unless an earlier matching route stops the evaluation.
</p>

<div class="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_24rem]">
	<div class="min-w-0">
		{#if loaded && routes.length === 0 && !draft}
			<p class="hint py-8">
				No routes yet, so nothing is sent anywhere.
				{#if lookups.connections.length === 0}
					Add a connection under <a href={resolve('/connections')}>Connections</a> first.
				{:else}
					<button type="button" class="text-accent underline" onclick={startNew}>Create the first route.</button>
				{/if}
			</p>
		{/if}

		<ol class="border-line border-t" aria-label="Routes">
			{#each routes as route, index (route.id)}
				<li class="border-line grid grid-cols-[2rem_1fr] items-baseline gap-x-3 gap-y-2 border-b px-2 py-2.5 md:grid-cols-[2rem_1fr_auto]">
					<span class="text-faint">{index + 1}</span>
					<div class="min-w-0">
						<div class="flex flex-wrap items-baseline gap-x-3">
							<span class="font-medium {route.enabled ? '' : 'text-muted line-through'}">{route.name}</span>
							{#if !route.enabled}<span class="tag">off</span>{/if}
							{#if route.stop}<span class="tag" title="Routes below are skipped when this one matches">stops here</span>{/if}
						</div>
						<div class="text-muted break-words">{summarize(route.match, lookups.sourceNames)}</div>
						<div class="text-muted break-words">
							to
							{#each route.targets as target, i (i)}{i > 0 ? ', ' : ''}<span
									class={lookups.connectionNames[target.connection_id] ? 'text-fg' : 'problem'}
									>{lookups.connectionNames[target.connection_id] ?? 'a deleted connection'}</span
								>{target.destination ? ` (${target.destination})` : ''}{target.template_id
									? ` worded by ${templateNames[target.template_id] ?? 'a deleted template'}`
									: ''}{/each}
						</div>
						{#if summarizeChain(route.escalation, lookups.connectionNames)}
							<div class="text-muted break-words" data-testid="chain-summary">
								{summarizeChain(route.escalation, lookups.connectionNames)}
							</div>
						{/if}
						{#if summarizeShaping(route.shaping)}
							<div class="text-muted break-words" data-testid="shaping-summary">{summarizeShaping(route.shaping)}</div>
						{/if}
						{#if route.last_error}
							<div class="problem break-words">
								Expression failed{route.last_error_at ? ` ${ago(route.last_error_at)}` : ''}: {route.last_error}
							</div>
						{/if}
					</div>
					<div class="col-start-2 flex flex-wrap gap-1 md:col-start-3 md:justify-end">
						<button type="button" class="btn h-7 px-2" disabled={index === 0} onclick={() => move(route, -1)}
							aria-label="Move {route.name} up">Up</button
						>
						<button
							type="button"
							class="btn h-7 px-2"
							disabled={index === routes.length - 1}
							onclick={() => move(route, 1)}
							aria-label="Move {route.name} down">Down</button
						>
						<button type="button" class="btn h-7 px-2" onclick={() => toggle(route)}>
							{route.enabled ? 'Turn off' : 'Turn on'}
						</button>
						<button type="button" class="btn h-7 px-2" onclick={() => startEdit(route)}>Edit</button>
						<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (deleting = route)}>Delete</button>
					</div>
				</li>
			{/each}
		</ol>

		{#if draft}
			<form id="route-form" class="panel mt-6 flex scroll-mt-16 flex-col gap-5 p-4" onsubmit={save}>
				<h2>{draft.id ? 'Edit route' : 'New route'}</h2>
				<div class="max-w-md">
					<label class="field-label" for="route-name">Name</label>
					<input id="route-name" class="input" required maxlength="100" bind:value={draft.name} />
				</div>

				<fieldset class="flex flex-col gap-3">
					<legend class="mb-2 font-medium">Which events</legend>
					<MatchEditor bind:match={draft.match} sources={lookups.sources} />
					<p class="text-muted break-words">Matches: {summarize(cleanMatch(draft.match), lookups.sourceNames)}</p>
				</fieldset>

				<fieldset class="flex flex-col gap-2">
					<legend class="mb-2 font-medium">Send to</legend>
					<TargetRows
						bind:targets={draft.targets}
						connections={lookups.connections}
						{templates}
						{destinationLabel}
						header
					/>
					{#if lookups.connections.length === 0}
						<p class="hint">
							There are no connections. Add one under <a href={resolve('/connections')}>Connections</a>.
						</p>
					{/if}
					{#if templates.length === 0}
						<p class="hint">
							To word messages differently, write a template under <a href={resolve('/templates')}>Templates</a>.
						</p>
					{/if}
				</fieldset>

				<fieldset class="flex flex-col gap-3">
					<legend class="mb-1 font-medium">If nobody acknowledges</legend>
					<EscalationEditor
						bind:steps={draft.escalation}
						connections={lookups.connections}
						{templates}
						{destinationLabel}
					/>
				</fieldset>

				<fieldset class="flex flex-col gap-3">
					<legend class="mb-1 font-medium">Noise control</legend>
					<p class="hint max-w-2xl">
						Without any of these, every matching event is sent at once.
						{#if summarizeShaping(formToShaping(draft.shaping))}
							<span class="text-fg">Now: {summarizeShaping(formToShaping(draft.shaping))}.</span>
						{/if}
					</p>
					<ShapingEditor bind:form={draft.shaping} />
				</fieldset>

				<div class="flex flex-col gap-2">
					<label class="flex items-center gap-2">
						<input type="checkbox" bind:checked={draft.enabled} /> Route is on
					</label>
					<label class="flex items-center gap-2">
						<input type="checkbox" bind:checked={draft.stop} /> Skip the routes below when this one matches
					</label>
				</div>

				{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
				<div class="flex gap-2">
					<button type="submit" class="btn btn-primary" disabled={busy}>{draft.id ? 'Save route' : 'Create route'}</button>
					<button type="button" class="btn" onclick={() => (draft = null)}>Cancel</button>
				</div>
			</form>
		{/if}
	</div>

	{#if loaded}
		<div class="min-w-0 xl:sticky xl:top-5">
			<RouteTester routes={tested} {draftIndex} />
		</div>
	{/if}
</div>

<Confirm
	open={deleting !== null}
	title="Delete this route?"
	action="Delete route"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	Events that only {deleting?.name} matched will no longer be sent anywhere.
</Confirm>
