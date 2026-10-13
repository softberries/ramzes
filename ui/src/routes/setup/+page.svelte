<script lang="ts">
	import { invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import SetupFlow from '#lib/components/SetupFlow.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// Whether the admin existed when this page was opened decides if step one is shown.
	// svelte-ignore state_referenced_locally
	const started = data.me !== null;
</script>

<div class="mx-auto max-w-3xl px-4 py-10">
	<h1 class="text-gold mb-2 text-2xl">Set up ramzes</h1>
	<p class="hint mb-10 max-w-xl">
		Five steps from an empty instance to a message in one of your channels. Each step unlocks the next.
	</p>
	<SetupFlow origin={page.url.origin} onadmin={invalidateAll} {started} />
</div>
