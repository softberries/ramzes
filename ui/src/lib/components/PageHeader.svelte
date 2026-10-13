<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		/** What the page is for, in a sentence under the title. */
		children?: Snippet;
		/** Buttons and controls that act on the whole page. */
		actions?: Snippet;
	}

	let { title, children, actions }: Props = $props();
</script>

<header class="mb-6 flex flex-wrap items-start gap-x-6 gap-y-3">
	<div class="mr-auto min-w-0">
		<h1>{title}</h1>
		{#if children}<p class="text-muted mt-1 max-w-2xl">{@render children()}</p>{/if}
	</div>
	{#if actions}<div class="flex flex-wrap items-center gap-2">{@render actions()}</div>{/if}
</header>
