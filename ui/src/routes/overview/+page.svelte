<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import { SEVERITIES, type Overview } from '#lib/api/types.ts';
	import DeliveryChart from '#lib/components/DeliveryChart.svelte';
	import Donut from '#lib/components/Donut.svelte';
	import EventChart from '#lib/components/EventChart.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import SeverityMark from '#lib/components/SeverityMark.svelte';
	import StatCard from '#lib/components/StatCard.svelte';
	import { timeOfDay } from '#lib/format.ts';
	import { PERIODS, fillBuckets, periodOf, proportion, shareWords, trend } from '#lib/overview.ts';
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
	/** Deliveries that reached an end, now and in the stretch before: the share that arrived is of these. */
	const settled = $derived(overview ? overview.deliveries_delivered + overview.deliveries_failed : 0);
	const settledBefore = $derived(
		overview ? overview.previous.deliveries_delivered + overview.previous.deliveries_failed : 0
	);
	const arrivedTrend = $derived.by(() => {
		if (!overview || settled === 0 || settledBefore === 0) return undefined;
		// Compared as shares of a thousand, so a tenth of a percent shows.
		return trend(
			Math.round((overview.deliveries_delivered / settled) * 1000),
			Math.round((overview.previous.deliveries_delivered / settledBefore) * 1000)
		);
	});
	/** How many events of each severity arrived, most severe first, those that did not occur left out. */
	const severities = $derived(
		[...SEVERITIES]
			.reverse()
			.map((severity) => ({ severity, events: buckets.reduce((sum, bucket) => sum + bucket[severity], 0) }))
			.filter((row) => row.events > 0)
	);
</script>

{#snippet ranked(title: string, rows: { name: string; events: number }[], empty: string, mono: boolean)}
	<section class="card" aria-label={title}>
		<div class="card-head"><h2>{title}</h2></div>
		<div class="card-body">
			{#if rows.length === 0}
				<p class="hint">{empty}</p>
			{:else}
				<ol class="flex flex-col gap-3">
					{#each rows as row (row.name)}
						<li>
							<div class="flex items-baseline gap-3">
								<span class="mr-auto min-w-0 truncate {mono ? 'mono' : ''}" title={row.name}>{row.name}</span>
								<span class="text-muted tabular-nums">{row.events}</span>
							</div>
							<div class="bg-raised mt-1 h-1.5 overflow-hidden rounded-full">
								<div class="bg-accent h-full rounded-full" style="width: {proportion(row.events, rows[0].events)}%"></div>
							</div>
						</li>
					{/each}
				</ol>
			{/if}
		</div>
	</section>
{/snippet}

<PageHeader title="Overview">
	What arrived, where it went and what needs someone's attention.
	{#snippet actions()}
		<label class="text-muted" for="overview-period">Period</label>
		<select id="overview-period" class="input w-auto" value={String(hours)} onchange={choose}>
			{#each PERIODS as p (p.hours)}<option value={String(p.hours)}>Last {p.label}</option>{/each}
		</select>
	{/snippet}
</PageHeader>

{#if overview && shown === hours}
	<div class="mb-5 grid grid-cols-2 gap-4 xl:grid-cols-4">
		<StatCard
			label="Events received"
			trend={trend(overview.events, overview.previous.events)}
			against="the {period} before"
			note="Nothing in the {period} before"
			spark={buckets.map((bucket) => bucket.events)}
		>
			<span data-testid="events-total">{overview.events}</span>
		</StatCard>
		<StatCard
			label="Deliveries that arrived"
			trend={arrivedTrend}
			against="the {period} before"
			more="good"
			note={settled === 0 ? 'Nothing was sent' : `${overview.deliveries_delivered} of ${settled} that ended`}
			spark={buckets.map((bucket) => bucket.delivered)}
			sparkColor="var(--st-delivered)"
		>
			<span data-testid="delivered-share">{shareWords(overview.deliveries_delivered, deliveries) || '–'}</span>
		</StatCard>
		<StatCard label="Notifications waiting now" note="Collecting events, or held back">
			<a href={resolve('/notifications')} data-testid="waiting-now">{overview.waiting}</a>
		</StatCard>
		<StatCard label="Sent, not acknowledged" note="Nobody has answered these yet">
			<a href={resolve('/notifications?view=sent')} data-testid="unacknowledged-now">{overview.unacknowledged}</a>
		</StatCard>
	</div>

	{#if overview.events === 0 && deliveries === 0}
		<p class="card hint mb-5 p-6" data-testid="overview-empty">
			Nothing has arrived in the last {period}. To send the first event, create a source under
			<a href={resolve('/sources')}>Sources</a> and use its example command.
		</p>
	{:else}
		<div class="mb-5 grid gap-4 xl:grid-cols-3">
			<section class="card xl:col-span-2" aria-label="Events received">
				<div class="card-head"><h2>Events received</h2></div>
				<div class="card-body"><EventChart {buckets} bucketMs={overview.bucket_ms} {now} /></div>
			</section>

			<section class="card" aria-label="Events by severity">
				<div class="card-head"><h2>By severity</h2></div>
				<div class="card-body">
					{#if severities.length === 0}
						<p class="hint">No event arrived.</p>
					{:else}
						<ol class="flex flex-col gap-3">
							{#each severities as row (row.severity)}
								<li>
									<div class="flex items-baseline gap-3">
										<span class="mr-auto"><SeverityMark severity={row.severity} label /></span>
										<span class="text-muted tabular-nums">{row.events}</span>
										<span class="text-faint w-12 text-right tabular-nums">{shareWords(row.events, overview.events)}</span>
									</div>
									<div class="bg-raised mt-1 h-1.5 overflow-hidden rounded-full">
										<div
											class="h-full rounded-full"
											style="width: {proportion(row.events, severities[0] ? Math.max(...severities.map((s) => s.events)) : 0)}%; background: var(--fill-{row.severity})"
										></div>
									</div>
								</li>
							{/each}
						</ol>
					{/if}
				</div>
			</section>
		</div>

		<div class="mb-5 grid gap-4 xl:grid-cols-3">
			<section class="card" aria-label="Deliveries">
				<div class="card-head"><h2>Deliveries</h2></div>
				<div class="card-body">
					{#if deliveries === 0}
						<p class="hint">
							No message was sent in the last {period}. Events go out when a route matches them: see
							<a href={resolve('/routes')}>Routes</a>.
						</p>
					{:else}
						<div class="flex flex-wrap items-center gap-6">
							<Donut
								parts={results.map((r) => ({ label: r.label, value: r.count, color: `var(--fill-${r.key})` }))}
								figure={shareWords(overview.deliveries_delivered, deliveries)}
								caption="arrived"
							/>
							<ul class="flex min-w-40 flex-1 flex-col gap-2">
								{#each results as result (result.key)}
									<li class="flex items-baseline gap-2" data-testid="deliveries-{result.key}">
										<span
											class="inline-block h-2.5 w-2.5 rounded-sm"
											style="background: var(--fill-{result.key})"
											aria-hidden="true"
										></span>
										<span class="mr-auto">{result.count} {result.label}</span>
										<span class="text-muted tabular-nums">{shareWords(result.count, deliveries)}</span>
									</li>
								{/each}
							</ul>
						</div>
						{#if overview.deliveries_failed > 0}
							<p class="mt-4"><a href={resolve('/failures')}>See what failed and send it again</a></p>
						{/if}
					{/if}
				</div>
			</section>

			<section class="card xl:col-span-2" aria-label="Deliveries over time">
				<div class="card-head">
					<h2>Deliveries over time</h2>
					<span class="text-muted">by when the message was queued</span>
				</div>
				<div class="card-body"><DeliveryChart {buckets} bucketMs={overview.bucket_ms} {now} /></div>
			</section>
		</div>

		<div class="mb-5 grid gap-4 md:grid-cols-2">
			{@render ranked('Noisiest sources', overview.sources, 'No source sent anything.', false)}
			{@render ranked('Noisiest topics', overview.topics, 'No event arrived.', true)}
		</div>
	{/if}

	<section class="card overflow-hidden" aria-label="Failing connections">
		<div class="card-head"><h2>Failing connections</h2></div>
		{#if overview.failing.length === 0}
			<p class="card-body text-ok">No connection failed a delivery in the last {period}, and none is paused.</p>
		{:else}
			<div class="overflow-x-auto">
				<table class="table">
					<thead>
						<tr>
							<th scope="col">Connection</th>
							<th scope="col" class="num">Failed</th>
							<th scope="col" class="num">Delivered</th>
							<th scope="col">State</th>
							<th scope="col">Last error</th>
						</tr>
					</thead>
					<tbody>
						{#each overview.failing as failing (failing.connection_id)}
							<tr>
								<td><a href={resolve('/connections')} class="font-medium">{failing.name ?? 'A deleted connection'}</a></td>
								<td class="num {failing.failed > 0 ? 'problem' : 'text-muted'}">{failing.failed} failed</td>
								<td class="num text-muted">{failing.delivered} delivered</td>
								<td>
									{#if failing.paused_until}
										<span class="badge" style="--tone: var(--st-failed)">paused until {timeOfDay(failing.paused_until, now)}</span>
									{:else}
										<span class="text-muted">sending</span>
									{/if}
								</td>
								<td class="text-muted max-w-md break-words">{failing.last_error ?? ''}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</section>
{/if}
