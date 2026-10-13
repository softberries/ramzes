<script lang="ts">
	import type { Bucket } from '#lib/api/types.ts';
	import { bucketLabel, peakIndex } from '#lib/overview.ts';
	import StackedBars from './StackedBars.svelte';

	interface Props {
		/** Every bucket of the period, oldest first, the empty ones included. Build it with `fillBuckets`. */
		buckets: Bucket[];
		/** The length of a bucket in milliseconds. */
		bucketMs: number;
		/** Current time in unix milliseconds. */
		now: number;
	}

	let { buckets, bucketMs, now }: Props = $props();

	/** Most severe at the bottom, where the eye compares heights best. */
	const SERIES = [
		{ key: 'critical', label: 'critical', color: 'var(--fill-critical)' },
		{ key: 'error', label: 'error', color: 'var(--fill-error)' },
		{ key: 'warning', label: 'warning', color: 'var(--fill-warning)' },
		{ key: 'info', label: 'info', color: 'var(--fill-info)' },
		{ key: 'debug', label: 'debug', color: 'var(--fill-debug)' }
	] as const;

	const peak = $derived(peakIndex(buckets));
	const most = $derived(peak === null ? 0 : buckets[peak].events);
	const unit = $derived(bucketMs < 86_400_000 ? 'hour' : 'day');
	const label = (bucket: Bucket) => bucketLabel(bucket.start, bucketMs, now);
	const count = (events: number) => `${events} ${events === 1 ? 'event' : 'events'}`;

	const summary = $derived(
		peak === null ? `No events in any ${unit}` : `Most in one ${unit}: ${count(most)}, ${label(buckets[peak])}`
	);
	const columns = $derived(
		buckets.map((bucket) => {
			const values = SERIES.map((series) => bucket[series.key]);
			// Events the server did not break down by severity are drawn as informational.
			values[3] += bucket.events - values.reduce((sum, value) => sum + value, 0);
			return { key: bucket.start, label: label(bucket), values };
		})
	);
</script>

<div>
	<p class="text-muted mb-4" data-testid="chart-summary">{summary}</p>
	<StackedBars {columns} series={[...SERIES]} noun={['event', 'events']} columnHeading="Start" />
</div>
