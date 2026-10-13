<script lang="ts">
	import type { NotificationRow, NotificationState } from '#lib/api/types.ts';
	import { stateWords } from '#lib/notifications.ts';

	interface Props {
		notification: NotificationRow;
		/** Current time in unix milliseconds. */
		now: number;
	}

	let { notification, now }: Props = $props();

	/** Waiting is gold, a message that went out is plain, a handled one recedes. */
	const TONES: Record<NotificationState, string> = {
		grouping: '--gold',
		held: '--gold',
		sent: '--fg',
		acknowledged: '--st-delivered',
		resolved: '--muted',
		suppressed: '--muted'
	};

	const words = $derived(stateWords(notification, now));
</script>

<span class="break-words" data-state={notification.state}>
	<span class="badge" style="--tone: var({TONES[notification.state]})">{words.state}</span>
	{#if words.when}<span class="text-muted inline-block" title={notification.acked_by ?? undefined}>{words.when}</span>{/if}
	{#if words.reason}<span class="text-muted mt-0.5 block">{words.reason}</span>{/if}
</span>
