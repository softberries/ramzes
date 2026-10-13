<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { api } from '#lib/api/client.ts';
	import type { NotificationDetail, NotificationRow } from '#lib/api/types.ts';
	import { canAcknowledge, canResolve } from '#lib/notifications.ts';
	import { silenceQuery } from '#lib/silences.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	interface Props {
		notification: NotificationRow;
		/** Called after the notification was acknowledged or resolved, or was found to have changed. */
		onchange: () => void;
	}

	let { notification, onchange }: Props = $props();

	let busy = $state(false);

	async function act(action: 'acknowledge' | 'resolve', done: string) {
		busy = true;
		try {
			await api('POST', `/api/notifications/${notification.id}/${action}`);
			toasts.ok(done);
		} catch (error) {
			// Most likely someone else got there first; the reload shows what it is now.
			toasts.problem(error);
		} finally {
			busy = false;
			onchange();
		}
	}

	/** Opens the silence form filled in from the event that opened this notification. */
	async function silenceLikeThis() {
		busy = true;
		try {
			const detail = await api<NotificationDetail>('GET', `/api/notifications/${notification.id}`);
			const first = detail.events[0];
			if (!first) {
				toasts.problem('This notification has no events left to copy conditions from.');
				return;
			}
			await goto(resolve(`/silences?${silenceQuery(first)}`));
		} catch (error) {
			toasts.problem(error);
		} finally {
			busy = false;
		}
	}
</script>

<!-- Tall enough for a thumb on a phone, compact beside a row on a wide screen. -->
<div class="flex flex-wrap gap-1.5">
	{#if canAcknowledge(notification.state)}
		<button
			type="button"
			class="btn btn-sm max-md:h-10 max-md:px-3 {notification.state === 'sent' ? 'btn-primary' : ''}"
			disabled={busy}
			aria-label="Acknowledge {notification.title}"
			onclick={() => act('acknowledge', 'Acknowledged.')}
		>
			Acknowledge
		</button>
	{/if}
	{#if canResolve(notification.state)}
		<button
			type="button"
			class="btn btn-sm max-md:h-10 max-md:px-3"
			disabled={busy}
			aria-label="Resolve {notification.title}"
			onclick={() => act('resolve', 'Resolved. Nothing was sent.')}
		>
			Resolve
		</button>
	{/if}
	<button
		type="button"
		class="btn btn-sm max-md:h-10 max-md:px-3"
		disabled={busy}
		aria-label="Silence events like {notification.title}"
		title="Silence events like this one"
		onclick={silenceLikeThis}
	>
		Silence
	</button>
</div>
