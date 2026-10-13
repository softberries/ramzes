<script lang="ts" module>
	/** One column of a [`DataTable`]. */
	export interface Column<Row> {
		key: string;
		label: string;
		/** The value rows are ordered by; a column without one cannot be the ordering. */
		sort?: (row: Row) => string | number | null | undefined;
		/** Largest or newest first on the first click, as numbers and times are usually wanted. */
		descendingFirst?: boolean;
		/** Line the column up on the right. */
		end?: boolean;
		/** Leave the column out below this width: `md` is a tablet, `lg` a desktop. */
		from?: 'md' | 'lg';
		/** A width for the column, as CSS. */
		width?: string;
	}
</script>

<script lang="ts" generics="Row">
	import type { Snippet } from 'svelte';
	import { nextSort, pageCount, pageRows, searchRows, sortRows, type Sort } from '#lib/table.ts';
	import Icon from './Icon.svelte';
	import Pagination from './Pagination.svelte';
	import SortHeader from './SortHeader.svelte';

	interface Props {
		/** Every row; the table searches, orders and pages them itself. */
		rows: Row[];
		columns: Column<Row>[];
		key: (row: Row) => string;
		/** What the table holds, for assistive technology and for the count: "connections". */
		noun: string;
		/** How one cell is drawn. */
		cell: Snippet<[Row, Column<Row>]>;
		/** The text of a row that the search looks in. Without it there is no search box. */
		search?: (row: Row) => string;
		searchPlaceholder?: string;
		/** The ordering before any heading is clicked. */
		initialSort?: Sort;
		pageSize?: number;
		/** Controls beside the search box: filters, or what acts on chosen rows. */
		toolbar?: Snippet;
		/** What to say when there are no rows at all. */
		empty?: Snippet;
		/** False while the rows are still on their way. */
		loaded?: boolean;
		/** Extra classes for a row, such as a highlight. */
		rowClass?: (row: Row) => string;
	}

	let {
		rows,
		columns,
		key,
		noun,
		cell,
		search,
		searchPlaceholder,
		initialSort,
		pageSize = 25,
		toolbar,
		empty,
		loaded = true,
		rowClass
	}: Props = $props();

	let query = $state('');
	// svelte-ignore state_referenced_locally
	let sort = $state<Sort | null>(initialSort ?? null);
	let page = $state(1);
	// svelte-ignore state_referenced_locally
	let size = $state(pageSize);

	const found = $derived(search ? searchRows(rows, search, query) : rows);
	const ordered = $derived.by(() => {
		const column = columns.find((c) => c.key === sort?.key);
		return column?.sort && sort ? sortRows(found, column.sort, sort.descending) : found;
	});
	/** The page shown: the one asked for, or the last one when the rows no longer reach that far. */
	const at = $derived(Math.min(page, pageCount(ordered.length, size)));
	const shown = $derived(pageRows(ordered, at, size));

	const FROM = { md: 'hidden md:table-cell', lg: 'hidden lg:table-cell' };
	const cellClass = (column: Column<Row>) =>
		`${column.end ? 'num' : ''} ${column.from ? FROM[column.from] : ''}`;

	function onsort(column: string) {
		sort = nextSort(sort, column, columns.find((c) => c.key === column)?.descendingFirst ?? false);
		page = 1;
	}
</script>

<div class="card overflow-hidden">
	{#if search || toolbar}
		<div class="card-head">
			{#if search}
				<label class="search">
					<Icon name="search" />
					<span class="sr-only">Search {noun}</span>
					<input
						type="search"
						class="input"
						placeholder={searchPlaceholder ?? `Search ${noun}`}
						bind:value={query}
						oninput={() => (page = 1)}
					/>
				</label>
			{/if}
			{#if toolbar}{@render toolbar()}{/if}
			<span class="text-muted ml-auto whitespace-nowrap" data-testid="count">
				{#if query.trim() !== ''}{found.length} of {rows.length} {noun}{:else}{rows.length} {noun}{/if}
			</span>
		</div>
	{/if}

	<div class="overflow-x-auto">
		<table class="table" aria-label={noun} aria-busy={!loaded}>
			<thead>
				<tr>
					{#each columns as column (column.key)}
						{#if column.sort}
							<SortHeader column={column.key} label={column.label} {sort} end={column.end} {onsort} />
						{:else}
							<th scope="col" class={cellClass(column)} style:width={column.width}>{column.label}</th>
						{/if}
					{/each}
				</tr>
			</thead>
			<tbody>
				{#each shown as row (key(row))}
					<tr class={rowClass?.(row)}>
						{#each columns as column (column.key)}
							<td class={cellClass(column)}>{@render cell(row, column)}</td>
						{/each}
					</tr>
				{:else}
					{#if loaded}
						<tr>
							<td colspan={columns.length} class="text-muted py-10 text-center">
								{#if rows.length > 0}
									No {noun} match “{query}”.
									<button type="button" class="text-accent underline" onclick={() => (query = '')}>Clear the search</button>
								{:else if empty}
									{@render empty()}
								{:else}
									No {noun} yet.
								{/if}
							</td>
						</tr>
					{/if}
				{/each}
			</tbody>
		</table>
	</div>

	{#if ordered.length > Math.min(size, pageSize)}
		<Pagination
			page={at}
			{size}
			total={ordered.length}
			{noun}
			onpage={(next) => (page = next)}
			onsize={(next) => {
				size = next;
				page = 1;
			}}
		/>
	{/if}
</div>

<style>
	.search {
		position: relative;
		display: flex;
		align-items: center;
		flex: 1 1 14rem;
		max-width: 24rem;
		color: var(--faint);
	}

	.search :global(svg) {
		position: absolute;
		left: 0.65rem;
		pointer-events: none;
	}

	.search .input {
		padding-left: 2.1rem;
	}
</style>
