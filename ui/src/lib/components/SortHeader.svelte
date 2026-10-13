<script lang="ts">
	import type { Sort } from '#lib/table.ts';
	import Icon from './Icon.svelte';

	interface Props {
		/** The column's key, as it appears in `sort`. */
		column: string;
		label: string;
		sort: Sort | null;
		/** Numbers and columns that end the row line up on the right. */
		end?: boolean;
		onsort: (column: string) => void;
	}

	let { column, label, sort, end = false, onsort }: Props = $props();

	const active = $derived(sort?.key === column);
	const direction = $derived(active ? (sort?.descending ? 'descending' : 'ascending') : 'none');
</script>

<th scope="col" aria-sort={direction} class={end ? 'num' : ''}>
	<button type="button" class="sort {active ? 'text-fg' : ''}" onclick={() => onsort(column)}>
		{label}
		<span class={active ? '' : 'idle'}>
			<Icon name={active ? (sort?.descending ? 'down' : 'up') : 'sort'} size={13} />
		</span>
	</button>
</th>

<style>
	.sort {
		display: inline-flex;
		align-items: center;
		gap: 0.3rem;
		font: inherit;
		color: inherit;
		cursor: pointer;
	}

	.sort:hover {
		color: var(--fg);
	}

	/* A column that is not the ordering shows that it could be, faintly. */
	.idle {
		color: var(--faint);
		opacity: 0.7;
	}
</style>
