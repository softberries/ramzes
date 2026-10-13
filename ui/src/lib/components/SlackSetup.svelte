<script lang="ts">
	import { interactionsUrl } from '#lib/connectors.ts';
	import CopyBlock from './CopyBlock.svelte';

	/** The id of a Slack app connection. */
	let { connectionId }: { connectionId: string } = $props();
</script>

<div class="flex max-w-2xl flex-col gap-2">
	<p>Slack sends button presses to this address:</p>
	<CopyBlock text={interactionsUrl(location.origin, connectionId)} testid="interactions-url" />
	<p class="hint">
		In your Slack app's settings, open Interactivity &amp; Shortcuts, turn Interactivity on and paste the address as
		the Request URL.
	</p>
	<p class="hint">The buttons also need the signing secret to be set on this connection.</p>
</div>
