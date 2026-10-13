<script lang="ts">
	import type { Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import { PAGES } from '#lib/pages.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { theme } from '#lib/theme.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import Palette from './Palette.svelte';

	let { email, children }: { email: string; children: Snippet } = $props();

	let paletteOpen = $state(false);

	const LIVE_LABELS = { live: 'Live', connecting: 'Connecting', reconnecting: 'Reconnecting' } as const;
	const liveLabel = $derived(LIVE_LABELS[live.status]);

	function isCurrent(href: string): boolean {
		const path = page.url.pathname;
		// An event's own page belongs to the feed.
		return href === '/' ? path === '/' || path.startsWith('/events/') : path.startsWith(href);
	}

	async function signOut() {
		try {
			await api('POST', '/api/auth/logout');
			await goto(resolve('/login'), { refreshAll: true });
		} catch (error) {
			toasts.problem(error);
		}
	}

	function onKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			paletteOpen = !paletteOpen;
		}
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div class="min-h-dvh md:grid md:grid-cols-[13rem_1fr]">
	<aside
		class="border-line bg-surface flex flex-col border-b md:sticky md:top-0 md:h-dvh md:border-r md:border-b-0"
	>
		<a href={resolve('/')} class="text-gold flex items-center gap-2 px-4 py-4 text-base font-semibold no-underline">
			<svg viewBox="0 0 32 32" class="h-5 w-5" aria-hidden="true">
				<path d="M16 5 L28 26 H4 Z" fill="none" stroke="currentColor" stroke-width="2.5" />
				<path d="M16 15 L22 26 H10 Z" fill="currentColor" />
			</svg>
			ramzes
		</a>

		<nav class="flex overflow-x-auto md:flex-col" aria-label="Main">
			{#each PAGES as item (item.href)}
				<a
					href={resolve(item.href)}
					aria-current={isCurrent(item.href) ? 'page' : undefined}
					class="border-l-2 px-4 py-2 whitespace-nowrap no-underline {isCurrent(item.href)
						? 'border-gold bg-raised text-fg'
						: 'text-muted hover:text-fg border-transparent'}"
				>
					{item.label}
				</a>
			{/each}
		</nav>

		<div class="border-line mt-auto hidden flex-col gap-1 border-t p-3 md:flex">
			<div class="flex items-center gap-2 px-1 py-1" title="Connection to live updates">
				<span
					class="h-2 w-2 rounded-full {live.status === 'live' ? 'bg-ok' : 'bg-gold'}"
					aria-hidden="true"
				></span>
				<span class={live.status === 'live' ? 'text-fg' : 'text-gold'}>{liveLabel}</span>
			</div>
			<button type="button" class="btn btn-quiet justify-start px-1" onclick={() => (paletteOpen = true)}>
				Go to… <span class="text-faint ml-auto">⌘K</span>
			</button>
			<button type="button" class="btn btn-quiet justify-start px-1" onclick={() => theme.toggle()}>
				{theme.current === 'dark' ? 'Light theme' : 'Dark theme'}
			</button>
			<button type="button" class="btn btn-quiet justify-start px-1" onclick={signOut}>Sign out</button>
			<div class="text-faint truncate px-1 pt-1" title={email}>{email}</div>
		</div>
	</aside>

	<main class="min-w-0 px-4 py-5 md:px-8">
		{@render children()}
	</main>
</div>

<Palette bind:open={paletteOpen} onnavigate={(href) => goto(resolve(href))} />
