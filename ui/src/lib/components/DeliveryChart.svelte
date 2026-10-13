<script lang="ts">
	import type { Bucket } from '#lib/api/types.ts';
	import { bucketLabel } from '#lib/overview.ts';
	import StackedBars from './StackedBars.svelte';

	interface Props {
		/** Every bucket of the period, oldest first, the empty ones included. */
		buckets: Bucket[];
		bucketMs: number;
		/** Current time in unix milliseconds. */
		now: number;
	}

	let { buckets, bucketMs, now }: Props = $props();

	const SERIES = [
		{ label: 'failed', color: 'var(--fill-failed)' },
		{ label: 'delivered', color: 'var(--fill-delivered)' }
	];

	const columns = $derived(
		buckets.map((bucket) => ({
			key: bucket.start,
			label: bucketLabel(bucket.start, bucketMs, now),
			values: [bucket.failed, bucket.delivered]
		}))
	);
</script>

<StackedBars {columns} series={SERIES} noun={['delivery', 'deliveries']} testid="delivery-bar" />
