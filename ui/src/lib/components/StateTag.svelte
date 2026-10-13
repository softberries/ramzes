<script lang="ts">
	import type { DeliveryState, RowOutcome } from '#lib/api/types.ts';

	let { state }: { state: DeliveryState | RowOutcome } = $props();

	const WORDS: Record<DeliveryState | RowOutcome, string> = {
		pending: 'pending',
		in_progress: 'sending',
		delivered: 'delivered',
		failed: 'failed',
		dead: 'gave up',
		unrouted: 'no route',
		waiting: 'waiting',
		not_sent: 'not sent'
	};
</script>

<span class="whitespace-nowrap" style="color: var(--st-{state})" data-state={state}>{WORDS[state]}</span>
