<script lang="ts">
	import type { Bucket } from '#lib/api/types.ts';
	import { bucketLabel, labelSide, peakIndex, proportion } from '#lib/overview.ts';

	interface Props {
		/** Every bucket of the period, oldest first, the empty ones included. Build it with `fillBuckets`. */
		buckets: Bucket[];
		/** The length of a bucket in milliseconds. */
		bucketMs: number;
		/** Current time in unix milliseconds. */
		now: number;
	}

	let { buckets, bucketMs, now }: Props = $props();

	const peak = $derived(peakIndex(buckets));
	const most = $derived(peak === null ? 0 : buckets[peak].events);
	const unit = $derived(bucketMs < 86_400_000 ? 'hour' : 'day');
	const label = (bucket: Bucket) => bucketLabel(bucket.start, bucketMs, now);
	const count = (events: number) => `${events} ${events === 1 ? 'event' : 'events'}`;

	const summary = $derived(
		peak === null ? `No events in any ${unit}` : `Most in one ${unit}: ${count(most)}, ${label(buckets[peak])}`
	);
</script>

<figure>
	<figcaption class="text-muted mb-2" data-testid="chart-summary">{summary}</figcaption>
	<!--
		One bar for each hour or day. Pointing at a bar shows its number over it; the table below holds the
		numbers for anyone who cannot see the bars.
	-->
	<div class="bars" aria-hidden="true">
		{#each buckets as bucket, index (bucket.start)}
			{@const side = labelSide(index, buckets.length)}
			<div class="slot" data-testid="bar" data-events={bucket.events}>
				{#if index === peak}<span class="over peak text-fg" data-side={side}>{most}</span>{/if}
				<span class="over tip text-fg" data-side={side}>{label(bucket)}: {count(bucket.events)}</span>
				{#if bucket.events > 0}
					<div class="bar" style="height: {proportion(bucket.events, most)}%"></div>
				{/if}
			</div>
		{/each}
	</div>
	{#if buckets.length > 0}
		<div class="text-muted mt-1 flex justify-between" aria-hidden="true">
			<span>{label(buckets[0])}</span>
			<span>{label(buckets[buckets.length - 1])}</span>
		</div>
	{/if}
	<table class="sr-only">
		<caption>Events per {unit}</caption>
		<thead><tr><th scope="col">Start</th><th scope="col">Events</th></tr></thead>
		<tbody>
			{#each buckets as bucket (bucket.start)}
				<tr><td>{label(bucket)}</td><td>{bucket.events}</td></tr>
			{/each}
		</tbody>
	</table>
</figure>

<style>
	.bars {
		display: flex;
		align-items: stretch;
		gap: 2px;
		height: 8rem;
		/* Room for the number over the tallest bar. */
		padding-top: 1.4rem;
		border-bottom: 1px solid var(--line);
	}

	/* The whole column answers the pointer, so a short or empty bar is as easy to point at as a tall one. */
	.slot {
		position: relative;
		display: flex;
		flex: 1 1 0;
		align-items: flex-end;
		min-width: 0;
	}

	.slot:hover {
		background: var(--raised);
	}

	.bar {
		width: 100%;
		/* A bar stays a bar when there are only a few columns. */
		max-width: 2.5rem;
		min-height: 2px;
		margin: 0 auto;
		border-radius: 2px 2px 0 0;
		background: var(--accent);
	}

	/* A label over a column: the peak's number, or the number of the column pointed at. */
	.over {
		position: absolute;
		bottom: 100%;
		left: 50%;
		transform: translateX(-50%);
		white-space: nowrap;
	}

	/* Near an edge of the chart the label lines up with its bar's edge instead, so it stays inside. */
	.over[data-side='start'] {
		left: max(0px, 50% - 1.25rem);
		transform: none;
	}

	.over[data-side='end'] {
		right: max(0px, 50% - 1.25rem);
		left: auto;
		transform: none;
	}

	.tip {
		display: none;
	}

	.slot:hover .tip {
		display: block;
	}

	/* The pointed column's label takes the place of the peak's. */
	.bars:hover .peak {
		visibility: hidden;
	}
</style>
