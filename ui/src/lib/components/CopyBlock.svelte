<script lang="ts">
	let { text, testid }: { text: string; testid?: string } = $props();

	let copied = $state(false);

	async function copy() {
		await navigator.clipboard.writeText(text);
		copied = true;
		setTimeout(() => (copied = false), 2000);
	}
</script>

<div class="relative">
	<pre class="code pr-20" data-testid={testid}>{text}</pre>
	<button type="button" class="btn absolute top-2 right-2 h-7 px-2" onclick={copy}>
		{copied ? 'Copied' : 'Copy'}
	</button>
</div>
