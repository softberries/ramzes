<script lang="ts">
	import type { DeliveryState, Outcome } from '#lib/api/types.ts';

	let { state }: { state: DeliveryState | Outcome } = $props();

	const WORDS: Record<DeliveryState | Outcome, string> = {
		pending: 'pending',
		in_progress: 'sending',
		delivered: 'delivered',
		failed: 'failed',
		dead: 'gave up',
		unrouted: 'no route'
	};
</script>

<span class="whitespace-nowrap" style="color: var(--st-{state})" data-state={state}>{WORDS[state]}</span>
