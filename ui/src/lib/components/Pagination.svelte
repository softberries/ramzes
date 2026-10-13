<script lang="ts">
	import { pageCount, rangeWords } from '#lib/table.ts';
	import Icon from './Icon.svelte';

	interface Props {
		/** The page shown, counted from 1. */
		page: number;
		size: number;
		/** Rows in all, over every page. */
		total: number;
		/** The sizes offered; the control is left out when there is one or none. */
		sizes?: number[];
		/** What the rows are, for the label of the controls: "events". */
		noun?: string;
		onpage: (page: number) => void;
		onsize?: (size: number) => void;
	}

	let { page, size, total, sizes = [25, 50, 100], noun = 'rows', onpage, onsize }: Props = $props();

	const pages = $derived(pageCount(total, size));
</script>

<nav class="pager text-muted" aria-label="Pages of {noun}">
	<span data-testid="range">{rangeWords(page, size, total)}</span>
	{#if onsize && sizes.length > 1}
		<label class="ml-auto flex items-center gap-2">
			<span>Per page</span>
			<select
				class="input h-8 w-auto"
				value={String(size)}
				onchange={(event) => onsize(Number((event.currentTarget as HTMLSelectElement).value))}
			>
				{#each sizes as option (option)}<option value={String(option)}>{option}</option>{/each}
			</select>
		</label>
	{/if}
	<div class="flex items-center gap-1 {onsize && sizes.length > 1 ? '' : 'ml-auto'}">
		<button type="button" class="btn btn-sm btn-icon" aria-label="First page" disabled={page <= 1} onclick={() => onpage(1)}>
			<Icon name="first" />
		</button>
		<button
			type="button"
			class="btn btn-sm btn-icon"
			aria-label="Previous page"
			disabled={page <= 1}
			onclick={() => onpage(page - 1)}
		>
			<Icon name="left" />
		</button>
		<span class="px-2 whitespace-nowrap" aria-current="page">Page {page} of {pages}</span>
		<button
			type="button"
			class="btn btn-sm btn-icon"
			aria-label="Next page"
			disabled={page >= pages}
			onclick={() => onpage(page + 1)}
		>
			<Icon name="right" />
		</button>
		<button
			type="button"
			class="btn btn-sm btn-icon"
			aria-label="Last page"
			disabled={page >= pages}
			onclick={() => onpage(pages)}
		>
			<Icon name="last" />
		</button>
	</div>
</nav>

<style>
	.pager {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 1rem;
		padding: 0.6rem 1.1rem;
		border-top: 1px solid var(--line);
		font-size: 0.93rem;
		font-variant-numeric: tabular-nums;
	}
</style>
