<script lang="ts">
	import { PAGES, type PageHref } from '#lib/pages.ts';
	import Icon from './Icon.svelte';

	let { open = $bindable(false), onnavigate }: { open?: boolean; onnavigate: (href: PageHref) => void } = $props();

	let query = $state('');
	let selected = $state(0);
	let input = $state<HTMLInputElement>();

	const matches = $derived(PAGES.filter((p) => p.label.toLowerCase().includes(query.trim().toLowerCase())));

	$effect(() => {
		if (open) {
			query = '';
			selected = 0;
			input?.focus();
		}
	});

	function go(href: PageHref) {
		open = false;
		onnavigate(href);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			open = false;
		} else if (event.key === 'ArrowDown') {
			event.preventDefault();
			selected = Math.min(selected + 1, matches.length - 1);
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			selected = Math.max(selected - 1, 0);
		} else if (event.key === 'Enter' && matches[selected]) {
			event.preventDefault();
			go(matches[selected].href);
		}
	}
</script>

{#if open}
	<div class="fixed inset-0 z-40 flex items-start justify-center px-4 pt-[15vh]">
		<button
			type="button"
			class="absolute inset-0 cursor-default bg-black/50"
			aria-label="Close"
			onclick={() => (open = false)}
		></button>
		<div class="card relative w-full max-w-md overflow-hidden" role="dialog" aria-label="Go to a page">
			<label class="field border-line text-faint flex items-center gap-2.5 border-b px-3.5">
				<Icon name="search" />
				<input
					bind:this={input}
					bind:value={query}
					oninput={() => (selected = 0)}
					onkeydown={onKeydown}
					class="text-fg h-11 w-full bg-transparent"
					placeholder="Go to a page"
					aria-label="Go to a page"
				/>
			</label>
			<ul class="max-h-[60vh] overflow-y-auto p-1.5">
				{#each matches as item, index (item.href)}
					<li>
						<button
							type="button"
							class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left {index === selected
								? 'bg-raised text-fg'
								: 'text-muted'}"
							aria-current={index === selected ? 'true' : undefined}
							onclick={() => go(item.href)}
						>
							<Icon name={item.icon} />
							{item.label}
						</button>
					</li>
				{:else}
					<li class="hint px-2.5 py-2">No page matches “{query}”.</li>
				{/each}
			</ul>
		</div>
	</div>
{/if}

<style>
	/* The field fills the top of the card, so the card's edge shows where the typing goes. */
	.field input:focus-visible {
		outline: none;
	}

	.field:focus-within {
		border-bottom-color: var(--accent);
	}

	.field input::placeholder {
		color: var(--faint);
	}
</style>
