<script lang="ts">
	import type { Snippet } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { api } from '#lib/api/client.ts';
	import { NAV_GROUPS, currentPage } from '#lib/pages.ts';
	import { live } from '#lib/stream.svelte.ts';
	import { theme } from '#lib/theme.svelte.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import Icon from './Icon.svelte';
	import Palette from './Palette.svelte';

	let { email, children }: { email: string; children: Snippet } = $props();

	let paletteOpen = $state(false);
	/** Whether the navigation is unfolded on a narrow screen. Wide screens always show it. */
	let menuOpen = $state(false);

	const LIVE_LABELS = { live: 'Live', connecting: 'Connecting', reconnecting: 'Reconnecting' } as const;
	const liveLabel = $derived(LIVE_LABELS[live.status]);
	const current = $derived(currentPage(page.url.pathname)?.href);

	afterNavigate(() => {
		menuOpen = false;
	});

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
		} else if (event.key === 'Escape') {
			menuOpen = false;
		}
	}
</script>

<svelte:window onkeydown={onKeydown} />

<div class="min-h-dvh md:grid md:grid-cols-[15rem_1fr]">
	<!--
		A bar across the top on a phone, with the navigation folded behind the menu button. A rail on
		wide screens: the column is as tall as the page, and what it holds stays in view while the page scrolls.
	-->
	<aside class="border-line bg-surface sticky top-0 z-30 border-b md:static md:border-r md:border-b-0">
		<div class="md:sticky md:top-0 md:flex md:h-dvh md:flex-col md:overflow-y-auto">
			<div class="flex items-center gap-3 pr-3 md:pr-0">
				<a
					href={resolve('/')}
					class="text-fg mr-auto flex items-center gap-2.5 px-5 py-3 text-lg font-semibold tracking-tight no-underline md:py-5"
				>
					<svg viewBox="0 0 32 32" class="text-gold h-6 w-6" aria-hidden="true">
						<path d="M16 5 L28 26 H4 Z" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linejoin="round" />
						<path d="M16 15 L22 26 H10 Z" fill="currentColor" />
					</svg>
					ramzes
				</a>
				<div class="flex items-center gap-2 md:hidden" title="Connection to live updates">
					<span class="h-2 w-2 rounded-full {live.status === 'live' ? 'bg-ok' : 'bg-gold'}" aria-hidden="true"></span>
					<span class={live.status === 'live' ? 'text-muted' : 'text-gold'}>{liveLabel}</span>
				</div>
				<button
					type="button"
					class="btn md:hidden"
					aria-expanded={menuOpen}
					aria-controls="main-menu"
					onclick={() => (menuOpen = !menuOpen)}
				>
					{menuOpen ? 'Close menu' : 'Menu'}
				</button>
			</div>

			<div
				id="main-menu"
				class="border-line max-h-[calc(100dvh-3.5rem)] overflow-y-auto border-t md:flex md:max-h-none md:flex-1 md:flex-col md:overflow-visible md:border-t-0
					{menuOpen ? 'block' : 'hidden'}"
			>
				<button
					type="button"
					class="search border-line text-muted mx-3 mb-4 hidden h-9 items-center gap-2 rounded-md border px-2.5 text-left md:flex"
					onclick={() => (paletteOpen = true)}
				>
					<Icon name="search" />
					<span class="mr-auto">Go to…</span>
					<kbd class="text-faint">⌘K</kbd>
				</button>

				<nav class="grid grid-cols-2 gap-x-2 px-2 py-2 md:block md:px-3 md:py-0" aria-label="Main">
					{#each NAV_GROUPS as group, groupIndex (group.title)}
						<div class="md:mb-5">
							<div class="text-faint px-2.5 py-1.5 text-[0.86rem] font-medium" id="nav-group-{groupIndex}">
								{group.title}
							</div>
							<ul aria-labelledby="nav-group-{groupIndex}" class="flex flex-col gap-0.5">
								{#each group.pages as item (item.href)}
									<li>
										<a
											href={resolve(item.href)}
											aria-current={current === item.href ? 'page' : undefined}
											class="nav-link flex items-center gap-2.5 rounded-md px-2.5 py-2.5 whitespace-nowrap no-underline md:py-1.5 {current ===
											item.href
												? 'bg-raised text-fg font-medium'
												: 'text-muted hover:text-fg'}"
										>
											<Icon name={item.icon} />
											{item.label}
										</a>
									</li>
								{/each}
							</ul>
						</div>
					{/each}
				</nav>

				<!-- A row under the navigation on a phone; a column at the foot of the rail on wide screens. -->
				<div
					class="border-line flex flex-wrap items-center gap-x-1 border-t px-3 py-1 md:mt-auto md:flex-col md:items-stretch md:gap-0.5 md:py-3"
				>
					<div class="hidden items-center gap-2 px-2.5 py-1.5 md:flex" title="Connection to live updates">
						<span class="relative flex h-2 w-2" aria-hidden="true">
							<span class="h-2 w-2 rounded-full {live.status === 'live' ? 'bg-ok' : 'bg-gold'}"></span>
						</span>
						<span class={live.status === 'live' ? 'text-muted' : 'text-gold'}>{liveLabel}</span>
					</div>
					<button type="button" class="btn btn-quiet justify-start px-2.5" onclick={() => theme.toggle()}>
						<Icon name={theme.current === 'dark' ? 'sun' : 'moon'} />
						{theme.current === 'dark' ? 'Light theme' : 'Dark theme'}
					</button>
					<button type="button" class="btn btn-quiet justify-start px-2.5" onclick={signOut}>
						<Icon name="logout" />
						Sign out
					</button>
					<div class="text-faint ml-auto min-w-0 truncate px-2.5 md:ml-0 md:pt-1" title={email}>{email}</div>
				</div>
			</div>
		</div>
	</aside>

	<main class="min-w-0 px-4 py-6 md:px-10 md:py-8">
		<div class="mx-auto max-w-[92rem]">
			{@render children()}
		</div>
	</main>
</div>

<Palette bind:open={paletteOpen} onnavigate={(href) => goto(resolve(href))} />

<style>
	/* The page in view: its fill, and its icon in the brand colour. */
	.nav-link[aria-current='page'] :global(svg) {
		color: var(--gold);
	}

	.search {
		background: var(--field);
	}

	.search:hover {
		border-color: var(--faint);
	}
</style>
