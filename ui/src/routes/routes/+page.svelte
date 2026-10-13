<script lang="ts">
	import { resolve } from '$app/paths';
	import { api, messageOf } from '#lib/api/client.ts';
	import type { Route, RouteMatch } from '#lib/api/types.ts';
	import Confirm from '#lib/components/Confirm.svelte';
	import MatchEditor from '#lib/components/MatchEditor.svelte';
	import { lookups } from '#lib/lookups.svelte.ts';
	import { cleanMatch, moved, summarize } from '#lib/routes.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** The route being created or edited. `id` is null for a new one. */
	interface Draft {
		id: string | null;
		name: string;
		enabled: boolean;
		stop: boolean;
		match: RouteMatch;
		connectionIds: string[];
	}

	let routes = $state<Route[]>([]);
	let loaded = $state(false);
	let draft = $state<Draft | null>(null);
	let problem = $state('');
	let busy = $state(false);
	let deleting = $state<Route | null>(null);

	async function load() {
		routes = await api<Route[]>('GET', '/api/routes');
		loaded = true;
	}

	$effect(() => {
		Promise.all([load(), lookups.load()]).catch((error) => toasts.problem(error));
	});

	function startNew() {
		problem = '';
		draft = {
			id: null,
			name: '',
			enabled: true,
			stop: false,
			match: {},
			connectionIds: lookups.connections[0] ? [lookups.connections[0].id] : []
		};
	}

	function startEdit(route: Route) {
		problem = '';
		draft = {
			id: route.id,
			name: route.name,
			enabled: route.enabled,
			stop: route.stop,
			match: structuredClone($state.snapshot(route.match)) as RouteMatch,
			connectionIds: route.targets.map((t) => t.connection_id)
		};
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!draft) return;
		busy = true;
		problem = '';
		const body = {
			name: draft.name,
			enabled: draft.enabled,
			stop: draft.stop,
			match: cleanMatch(draft.match),
			targets: draft.connectionIds.filter((id) => id !== '').map((connection_id) => ({ connection_id }))
		};
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
			await api('PUT', `/api/routes/${route.id}`, { ...route, enabled: !route.enabled });
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

<ol class="border-line border-t">
	{#each routes as route, index (route.id)}
		<li class="border-line grid grid-cols-[2rem_1fr_auto] items-baseline gap-x-3 border-b px-2 py-2.5">
			<span class="text-faint">{index + 1}</span>
			<div class="min-w-0">
				<div class="flex flex-wrap items-baseline gap-x-3">
					<span class="font-medium {route.enabled ? '' : 'text-muted line-through'}">{route.name}</span>
					{#if !route.enabled}<span class="tag">off</span>{/if}
					{#if route.stop}<span class="tag" title="Routes below are skipped when this one matches">stops here</span>{/if}
				</div>
				<div class="text-muted">{summarize(route.match, lookups.sourceNames)}</div>
				<div class="text-muted">
					to
					{#each route.targets as target, i (i)}{i > 0 ? ', ' : ''}<span
							class={lookups.connectionNames[target.connection_id] ? 'text-fg' : 'problem'}
							>{lookups.connectionNames[target.connection_id] ?? 'a deleted connection'}</span
						>{/each}
				</div>
			</div>
			<div class="flex flex-wrap justify-end gap-1">
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
	<form class="panel mt-6 flex flex-col gap-5 p-4" onsubmit={save}>
		<h2>{draft.id ? 'Edit route' : 'New route'}</h2>
		<div class="max-w-md">
			<label class="field-label" for="route-name">Name</label>
			<input id="route-name" class="input" required maxlength="100" bind:value={draft.name} />
		</div>

		<fieldset class="flex flex-col gap-3">
			<legend class="mb-2 font-medium">Which events</legend>
			<MatchEditor bind:match={draft.match} sources={lookups.sources} />
			<p class="text-muted">Matches: {summarize(cleanMatch(draft.match), lookups.sourceNames)}</p>
		</fieldset>

		<fieldset class="flex flex-col gap-2">
			<legend class="mb-2 font-medium">Send to</legend>
			{#each draft.connectionIds, index}
				<div class="flex max-w-md gap-2">
					<select class="input" aria-label="Connection" bind:value={draft.connectionIds[index]} required>
						<option value="" disabled>Choose a connection</option>
						{#each lookups.connections as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
					</select>
					{#if draft.connectionIds.length > 1}
						<button
							type="button"
							class="btn btn-quiet"
							onclick={() => draft && (draft.connectionIds = draft.connectionIds.filter((_, i) => i !== index))}
						>
							Remove
						</button>
					{/if}
				</div>
			{/each}
			{#if lookups.connections.length === 0}
				<p class="hint">
					There are no connections. Add one under <a href={resolve('/connections')}>Connections</a>.
				</p>
			{:else}
				<div>
					<button type="button" class="btn" onclick={() => draft && (draft.connectionIds = [...draft.connectionIds, ''])}>
						Add another connection
					</button>
				</div>
			{/if}
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

<Confirm
	open={deleting !== null}
	title="Delete this route?"
	action="Delete route"
	onconfirm={remove}
	oncancel={() => (deleting = null)}
>
	Events that only {deleting?.name} matched will no longer be sent anywhere.
</Confirm>
