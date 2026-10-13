<script lang="ts">
	import type { DeliveryState, Outcome } from '#lib/api/types.ts';

	let { state }: { state: DeliveryState | Outcome } = $props();

	const WORDS: Record<DeliveryState | Outcome, string> = {
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

<span class="badge" style="color: var(--st-{state}); --tone: var(--st-{state})" data-state={state}>{WORDS[state]}</span>
