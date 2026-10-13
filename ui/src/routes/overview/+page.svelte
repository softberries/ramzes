<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import type { Overview } from '#lib/api/types.ts';
	import EventChart from '#lib/components/EventChart.svelte';
	import { timeOfDay } from '#lib/format.ts';
	import { PERIODS, fillBuckets, periodOf, proportion, shareWords } from '#lib/overview.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	/** Changes arriving close together cause one reload. */
	const REFRESH_DELAY_MS = 1000;

	const hours = $derived(periodOf(page.url.searchParams.get('hours')));
	const period = $derived(PERIODS.find((p) => p.hours === hours)?.label ?? '');

	let overview = $state<Overview | null>(null);
	/** The period the numbers on screen are for. */
	let shown = $state(0);
	let now = $state(Date.now());

	async function load() {
		const asked = hours;
		const answer = await api<Overview>('GET', `/api/overview?hours=${asked}`);
		// The period changed while this was loading; a newer request is on its way.
		if (asked !== hours) return;
		overview = answer;
		shown = asked;
		now = Date.now();
	}

	$effect(() => {
		void hours;
		load().catch((error) => toasts.problem(error));
	});

	$effect(() => {
		let timer: ReturnType<typeof setTimeout> | undefined;
		const soon = () => {
			clearTimeout(timer);
			timer = setTimeout(() => load().catch(() => {}), REFRESH_DELAY_MS);
		};
		const offs = [
			live.on('event', soon),
			live.on('delivery', soon),
			live.on('notification', soon),
			live.on('resync', soon)
		];
		return () => {
			clearTimeout(timer);
			offs.forEach((off) => off());
		};
	});

	function choose(event: Event) {
		const chosen = (event.currentTarget as HTMLSelectElement).value;
		goto(resolve(chosen === String(PERIODS[0].hours) ? '/overview' : `/overview?hours=${chosen}`), {
			replace: true,
			reset: false
		});
	}

	const buckets = $derived(overview ? fillBuckets(overview.buckets, overview.since, overview.bucket_ms, now) : []);
	const deliveries = $derived(
		overview ? overview.deliveries_delivered + overview.deliveries_failed + overview.deliveries_pending : 0
	);
	/** The three delivery results, in the order they are drawn. */
	const results = $derived(
		overview
			? [
					{ key: 'delivered', label: 'delivered', count: overview.deliveries_delivered },
					{ key: 'failed', label: 'failed', count: overview.deliveries_failed },
					{ key: 'pending', label: 'pending', count: overview.deliveries_pending }
				]
			: []
	);
</script>

{#snippet ranked(title: string, rows: { name: string; events: number }[], empty: string)}
	<section aria-label={title}>
		<h2 class="mb-2">{title}</h2>
		{#if rows.length === 0}
			<p class="hint">{empty}</p>
		{:else}
			<ol class="flex flex-col gap-2">
				{#each rows as row (row.name)}
					<li>
						<div class="flex items-baseline gap-3">
							<span class="mr-auto min-w-0 truncate" title={row.name}>{row.name}</span>
							<span class="text-muted whitespace-nowrap">{row.events}</span>
						</div>
						<div class="bg-raised mt-0.5 h-1.5">
							<div class="bg-accent h-full" style="width: {proportion(row.events, rows[0].events)}%"></div>
						</div>
					</li>
				{/each}
			</ol>
		{/if}
	</section>
{/snippet}

<div class="mb-5 flex flex-wrap items-center gap-3">
	<h1 class="mr-auto">Overview</h1>
	<label class="text-muted" for="overview-period">Period</label>
	<select id="overview-period" class="input w-auto" value={String(hours)} onchange={choose}>
		{#each PERIODS as p (p.hours)}<option value={String(p.hours)}>Last {p.label}</option>{/each}
	</select>
</div>

{#if overview && shown === hours}
	<dl class="border-line bg-line mb-8 grid grid-cols-2 gap-px border md:grid-cols-4">
		<div class="bg-surface flex flex-col justify-between p-3">
			<dt class="text-muted">Events received</dt>
			<dd class="figure" data-testid="events-total">{overview.events}</dd>
		</div>
		<div class="bg-surface flex flex-col justify-between p-3">
			<dt class="text-muted">Deliveries that arrived</dt>
			<dd class="figure" data-testid="delivered-share">
				{shareWords(overview.deliveries_delivered, deliveries) || '–'}
			</dd>
		</div>
		<div class="bg-surface flex flex-col justify-between p-3">
			<dt class="text-muted">Notifications waiting now</dt>
			<dd class="figure">
				<a href={resolve('/notifications')} data-testid="waiting-now">{overview.waiting}</a>
			</dd>
		</div>
		<div class="bg-surface flex flex-col justify-between p-3">
			<dt class="text-muted">Sent, not acknowledged</dt>
			<dd class="figure">
				<a href={resolve('/notifications?view=sent')} data-testid="unacknowledged-now">{overview.unacknowledged}</a>
			</dd>
		</div>
	</dl>

	{#if overview.events === 0 && deliveries === 0}
		<p class="hint mb-8 max-w-2xl" data-testid="overview-empty">
			Nothing has arrived in the last {period}. To send the first event, create a source under
			<a href={resolve('/sources')}>Sources</a> and use its example command.
		</p>
	{:else}
		<section class="mb-8" aria-label="Events received">
			<h2 class="mb-2">Events received</h2>
			<EventChart {buckets} bucketMs={overview.bucket_ms} {now} />
		</section>

		<section class="mb-8 max-w-3xl" aria-label="Deliveries">
			<h2 class="mb-2">Deliveries</h2>
			{#if deliveries === 0}
				<p class="hint">
					No message was sent in the last {period}. Events go out when a route matches them: see
					<a href={resolve('/routes')}>Routes</a>.
				</p>
			{:else}
				<div class="flex h-2.5 gap-0.5" aria-hidden="true">
					{#each results.filter((r) => r.count > 0) as result (result.key)}
						<div style="flex: {result.count} 1 0; min-width: 2px; background: var(--st-{result.key})"></div>
					{/each}
				</div>
				<ul class="mt-2 flex flex-wrap gap-x-6 gap-y-1">
					{#each results as result (result.key)}
						<li class="flex items-baseline gap-2" data-testid="deliveries-{result.key}">
							<span class="inline-block h-2 w-2" style="background: var(--st-{result.key})" aria-hidden="true"></span>
							<span>{result.count} {result.label}</span>
							<span class="text-muted">{shareWords(result.count, deliveries)}</span>
						</li>
					{/each}
				</ul>
				{#if overview.deliveries_failed > 0}
					<p class="mt-2"><a href={resolve('/failures')}>See what failed and send it again</a></p>
				{/if}
			{/if}
		</section>

		<div class="mb-8 grid max-w-5xl gap-8 md:grid-cols-2">
			{@render ranked('Noisiest sources', overview.sources, 'No source sent anything.')}
			{@render ranked('Noisiest topics', overview.topics, 'No event arrived.')}
		</div>
	{/if}

	<section class="max-w-5xl" aria-label="Failing connections">
		<h2 class="mb-2">Failing connections</h2>
		{#if overview.failing.length === 0}
			<p class="text-ok">No connection failed a delivery in the last {period}, and none is paused.</p>
		{:else}
			<ul class="border-line border-t">
				{#each overview.failing as failing (failing.connection_id)}
					<li class="border-line border-b px-2 py-2">
						<div class="flex flex-wrap items-baseline gap-x-4">
							<a href={resolve('/connections')} class="font-medium">{failing.name ?? 'A deleted connection'}</a>
							<span class={failing.failed > 0 ? 'problem' : 'text-muted'}>{failing.failed} failed</span>
							<span class="text-muted">{failing.delivered} delivered</span>
							{#if failing.paused_until}
								<span class="problem">paused until {timeOfDay(failing.paused_until, now)}</span>
							{/if}
						</div>
						{#if failing.last_error}<div class="text-muted break-words">Last error: {failing.last_error}</div>{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}

<style>
	.figure {
		font-size: 1.6rem;
		font-weight: 600;
		line-height: 1.3;
	}
</style>
