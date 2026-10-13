<script lang="ts">
	import type { RequestPreview } from '#lib/api/types.ts';

	/** The request a connection sends, with secrets already masked by the server. */
	let { request }: { request: RequestPreview } = $props();
</script>

<div class="code" data-testid="request">
	<div><span class="text-accent">{request.method}</span> {request.url}</div>
	{#each request.headers as [name, value], index (index)}
		<div><span class="text-muted">{name}:</span> {value}</div>
	{/each}
	{#if request.body}<pre class="border-line mt-2 border-t pt-2 whitespace-pre-wrap">{request.body}</pre>{/if}
</div>
