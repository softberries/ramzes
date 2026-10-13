<script lang="ts">
	import { PAGES, type PageHref } from '#lib/pages.ts';

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
	<div class="fixed inset-0 z-40 flex items-start justify-center pt-[15vh]">
		<button
			type="button"
			class="absolute inset-0 cursor-default bg-black/50"
			aria-label="Close"
			onclick={() => (open = false)}
		></button>
		<div class="panel relative w-full max-w-md" role="dialog" aria-label="Go to a page">
			<input
				bind:this={input}
				bind:value={query}
				oninput={() => (selected = 0)}
				onkeydown={onKeydown}
				class="input border-line h-10 border-0 border-b"
				placeholder="Go to a page"
				aria-label="Go to a page"
			/>
			<ul>
				{#each matches as item, index (item.href)}
					<li>
						<button
							type="button"
							class="w-full px-3 py-2 text-left {index === selected ? 'bg-raised text-fg' : 'text-muted'}"
							aria-current={index === selected ? 'true' : undefined}
							onclick={() => go(item.href)}
						>
							{item.label}
						</button>
					</li>
				{:else}
					<li class="hint px-3 py-2">No page matches “{query}”.</li>
				{/each}
			</ul>
		</div>
	</div>
{/if}
