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

<div class="min-h-dvh md:grid md:grid-cols-[13rem_1fr]">
	<!--
		A bar across the top on a phone, with the navigation folded behind the menu button. A rail on
		wide screens: the column is as tall as the page, and what it holds stays in view while the page scrolls.
	-->
	<aside class="border-line bg-surface sticky top-0 z-30 border-b md:static md:border-r md:border-b-0">
		<div class="md:sticky md:top-0 md:flex md:h-dvh md:flex-col md:overflow-y-auto">
			<div class="flex items-center gap-3 pr-3 md:pr-0">
				<a
					href={resolve('/')}
					class="text-gold mr-auto flex items-center gap-2 px-4 py-3 text-base font-semibold no-underline md:py-4"
				>
					<svg viewBox="0 0 32 32" class="h-5 w-5" aria-hidden="true">
						<path d="M16 5 L28 26 H4 Z" fill="none" stroke="currentColor" stroke-width="2.5" />
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
				<nav class="grid grid-cols-2 gap-x-2 py-2 md:block md:py-0" aria-label="Main">
					{#each NAV_GROUPS as group, groupIndex (group.title)}
						<div class="md:mb-3">
							<div class="text-faint px-4 py-1.5" id="nav-group-{groupIndex}">{group.title}</div>
							<ul aria-labelledby="nav-group-{groupIndex}">
								{#each group.pages as item (item.href)}
									<li>
										<a
											href={resolve(item.href)}
											aria-current={current === item.href ? 'page' : undefined}
											class="block border-l-2 px-4 py-2.5 whitespace-nowrap no-underline md:py-1.5 {current === item.href
												? 'border-gold bg-raised text-fg'
												: 'text-muted hover:text-fg border-transparent'}"
										>
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
					class="border-line flex flex-wrap items-center gap-x-1 border-t px-3 py-1 md:mt-auto md:flex-col md:items-stretch md:gap-1 md:py-3"
				>
					<div class="hidden items-center gap-2 px-1 py-1 md:flex" title="Connection to live updates">
						<span class="h-2 w-2 rounded-full {live.status === 'live' ? 'bg-ok' : 'bg-gold'}" aria-hidden="true"></span>
						<span class={live.status === 'live' ? 'text-fg' : 'text-gold'}>{liveLabel}</span>
					</div>
					<button type="button" class="btn btn-quiet hidden justify-start px-1 md:flex" onclick={() => (paletteOpen = true)}>
						Go to… <span class="text-faint ml-auto">⌘K</span>
					</button>
					<button type="button" class="btn btn-quiet justify-start px-1" onclick={() => theme.toggle()}>
						{theme.current === 'dark' ? 'Light theme' : 'Dark theme'}
					</button>
					<button type="button" class="btn btn-quiet justify-start px-1" onclick={signOut}>Sign out</button>
					<div class="text-faint ml-auto min-w-0 truncate px-1 md:ml-0 md:pt-1" title={email}>{email}</div>
				</div>
			</div>
		</div>
	</aside>

	<main class="min-w-0 px-4 py-5 md:px-8">
		{@render children()}
	</main>
</div>

<Palette bind:open={paletteOpen} onnavigate={(href) => goto(resolve(href))} />
