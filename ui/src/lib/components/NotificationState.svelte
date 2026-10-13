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
		grouping: 'text-gold',
		held: 'text-gold',
		sent: 'text-fg',
		acknowledged: 'text-ok',
		resolved: 'text-muted',
		suppressed: 'text-muted'
	};

	const words = $derived(stateWords(notification, now));
</script>

<span class="break-words" data-state={notification.state}>
	<span class="font-medium {TONES[notification.state]}">{words.state}</span>
	{#if words.when}<span class="text-muted" title={notification.acked_by ?? undefined}>{words.when}</span>{/if}
	{#if words.reason}<span class="text-muted block">{words.reason}</span>{/if}
</span>
