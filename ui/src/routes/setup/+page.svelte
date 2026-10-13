<script lang="ts">
	import { refreshAll } from '$app/navigation';
	import { page } from '$app/state';
	import BrandMark from '#lib/components/BrandMark.svelte';
	import SetupFlow from '#lib/components/SetupFlow.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// Whether the admin existed when this page was opened decides if step one is shown.
	// svelte-ignore state_referenced_locally
	const started = data.me !== null;
</script>

<div class="mx-auto max-w-2xl px-4 py-10 md:py-16">
	<div class="mb-8 flex flex-col items-center gap-3 text-center">
		<BrandMark />
		<h1>Set up ramzes</h1>
		<p class="hint max-w-md">
			Five steps from an empty instance to a message in one of your channels. Each step unlocks the next.
		</p>
	</div>
	<SetupFlow origin={page.url.origin} onadmin={refreshAll} {started} />
</div>
