<script lang="ts" module>
	/** One kind of thing counted in each column, drawn in one colour. */
	export interface Series {
		label: string;
		/** A CSS colour, normally a token: `var(--sev-error)`. */
		color: string;
	}

	/** One column of the chart. */
	export interface Column {
		key: number | string;
		/** What the column covers: "11:00", "Oct 12". */
		label: string;
		/** One number per series, in the order of the series, bottom to top. */
		values: number[];
	}
</script>

<script lang="ts">
	import { compact, labelSide, niceMax } from '#lib/overview.ts';

	interface Props {
		columns: Column[];
		series: Series[];
		/** What is counted, singular and plural, for the totals: `['event', 'events']`. */
		noun: [string, string];
		/** The heading of the first column of the table behind the chart. */
		columnHeading?: string;
		/** The test id of each column. */
		testid?: string;
	}

	let { columns, series, noun, columnHeading = 'Start', testid = 'bar' }: Props = $props();

	const total = (column: Column) => column.values.reduce((sum, value) => sum + value, 0);
	const top = $derived(niceMax(Math.max(0, ...columns.map(total))));
	const counted = (count: number) => `${count} ${count === 1 ? noun[0] : noun[1]}`;
	/** Series that have anything to show; the legend and the pointer's label leave the others out. */
	const present = $derived(series.map((_, index) => columns.some((column) => column.values[index] > 0)));
	/** Three or four names under the columns, evenly spread, first and last always among them. */
	const named = $derived.by(() => {
		const last = columns.length - 1;
		if (last < 1) return columns.length === 1 ? [0] : [];
		const steps = Math.min(4, last);
		return [...new Set(Array.from({ length: steps + 1 }, (_, i) => Math.round((i * last) / steps)))];
	});
</script>

<figure>
	<!--
		One column for each hour or day. Pointing at a column shows its numbers; the table below holds
		them for anyone who cannot see the columns.
	-->
	<div class="chart" aria-hidden="true">
		<div class="scale text-faint">
			<span>{compact(top)}</span>
			<span>{compact(top / 2)}</span>
			<span>0</span>
		</div>
		<div class="plot">
			<div class="grid-line" style="top: 0"></div>
			<div class="grid-line" style="top: 50%"></div>
			<div class="columns">
				{#each columns as column, index (column.key)}
					{@const sum = total(column)}
					<div class="slot" data-testid={testid} data-events={sum}>
						<div class="tip" data-side={labelSide(index, columns.length)}>
							<div class="text-fg font-medium">{column.label}</div>
							<div class="text-muted">{counted(sum)}</div>
							{#each series as one, at (one.label)}
								{#if column.values[at] > 0}
									<div class="flex items-center gap-2">
										<span class="dot" style="background: {one.color}"></span>
										<span class="text-muted mr-auto">{one.label}</span>
										<span class="text-fg">{column.values[at]}</span>
									</div>
								{/if}
							{/each}
						</div>
						{#if sum > 0}
							<div class="bar" style="height: {(sum / top) * 100}%">
								{#each series as one, at (one.label)}
									{#if column.values[at] > 0}
										<div style="flex: {column.values[at]} 1 0; background: {one.color}"></div>
									{/if}
								{/each}
							</div>
						{/if}
					</div>
				{/each}
			</div>
		</div>
		<div></div>
		<div class="names text-muted">
			{#each named as index (index)}
				<span style="left: {columns.length > 1 ? (index / (columns.length - 1)) * 100 : 50}%" data-side={labelSide(index, columns.length)}>
					{columns[index].label}
				</span>
			{/each}
		</div>
	</div>

	{#if series.length > 1}
		<ul class="text-muted mt-3 flex flex-wrap gap-x-5 gap-y-1" aria-hidden="true">
			{#each series as one, at (one.label)}
				{#if present[at]}
					<li class="flex items-center gap-2"><span class="dot" style="background: {one.color}"></span>{one.label}</li>
				{/if}
			{/each}
		</ul>
	{/if}

	<table class="sr-only">
		<caption>{noun[1]} per column</caption>
		<thead>
			<tr>
				<th scope="col">{columnHeading}</th>
				<th scope="col">Total</th>
				{#if series.length > 1}{#each series as one (one.label)}<th scope="col">{one.label}</th>{/each}{/if}
			</tr>
		</thead>
		<tbody>
			{#each columns as column (column.key)}
				<tr>
					<td>{column.label}</td>
					<td>{total(column)}</td>
					{#if series.length > 1}{#each column.values as value, at (at)}<td>{value}</td>{/each}{/if}
				</tr>
			{/each}
		</tbody>
	</table>
</figure>

<style>
	.chart {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 0.35rem 0.6rem;
	}

	.scale {
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		align-items: flex-end;
		height: 11rem;
		font-size: 0.86rem;
		font-variant-numeric: tabular-nums;
		line-height: 1;
	}

	.plot {
		position: relative;
		height: 11rem;
		border-bottom: 1px solid var(--line);
	}

	.grid-line {
		position: absolute;
		right: 0;
		left: 0;
		border-top: 1px dashed color-mix(in srgb, var(--line) 80%, transparent);
	}

	.columns {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: stretch;
		gap: 3px;
	}

	/* The whole column answers the pointer, so a short or empty one is as easy to point at as a tall one. */
	.slot {
		position: relative;
		display: flex;
		flex: 1 1 0;
		align-items: flex-end;
		min-width: 0;
		border-radius: 3px 3px 0 0;
	}

	.slot:hover {
		background: color-mix(in srgb, var(--raised) 70%, transparent);
	}

	.bar {
		display: flex;
		flex-direction: column-reverse;
		gap: 1px;
		width: 100%;
		/* A column stays a column when there are only a few. */
		max-width: 2.75rem;
		min-height: 3px;
		margin: 0 auto;
		overflow: hidden;
		border-radius: 3px 3px 0 0;
	}

	.dot {
		display: inline-block;
		width: 0.55rem;
		height: 0.55rem;
		flex: none;
		border-radius: 2px;
	}

	/* The numbers of the column pointed at, on a card over it. */
	.tip {
		position: absolute;
		z-index: 5;
		bottom: calc(100% + 0.4rem);
		left: 50%;
		display: none;
		min-width: 9rem;
		padding: 0.5rem 0.65rem;
		border: 1px solid var(--line);
		border-radius: var(--radius-control);
		background: var(--surface);
		box-shadow: var(--shadow);
		font-size: 0.93rem;
		white-space: nowrap;
		transform: translateX(-50%);
		pointer-events: none;
	}

	/* Near an edge of the chart the card lines up with that edge of its column, so it stays inside. */
	.tip[data-side='start'] {
		left: 0;
		transform: none;
	}

	.tip[data-side='end'] {
		right: 0;
		left: auto;
		transform: none;
	}

	.slot:hover .tip {
		display: block;
	}

	.names {
		position: relative;
		height: 1.3rem;
		font-size: 0.86rem;
	}

	.names span {
		position: absolute;
		white-space: nowrap;
		transform: translateX(-50%);
	}

	.names span[data-side='start'] {
		transform: none;
	}

	.names span[data-side='end'] {
		transform: translateX(-100%);
	}
</style>
