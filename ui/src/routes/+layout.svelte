<script lang="ts">
	import '@fontsource-variable/ibm-plex-sans/wght.css';
	import '@fontsource/ibm-plex-mono/400.css';
	import '@fontsource/ibm-plex-mono/400-italic.css';
	import '@fontsource/ibm-plex-mono/500.css';
	import '@fontsource/ibm-plex-mono/600.css';
	import '../app.css';

	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { onUnauthorized } from '#lib/api/client.ts';
	import Shell from '#lib/components/Shell.svelte';
	import Toasts from '#lib/components/Toasts.svelte';
	import favicon from '#lib/assets/favicon.svg';
	import { live } from '#lib/stream.svelte.ts';
	import { theme } from '#lib/theme.svelte.ts';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	// Setup and sign-in are full-page; everything else sits inside the navigation shell.
	const standalone = $derived(page.url.pathname === '/setup' || page.url.pathname === '/login');

	$effect(() => {
		theme.init();
		onUnauthorized(() => goto(resolve('/login')));
		return () => onUnauthorized(null);
	});

	$effect(() => {
		if (data.me) live.connect();
		else live.close();
	});
</script>

<svelte:head>
	<title>ramzes</title>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if standalone || !data.me}
	{@render children()}
{:else}
	<Shell email={data.me.email ?? ''}>
		{@render children()}
	</Shell>
{/if}
<Toasts />
