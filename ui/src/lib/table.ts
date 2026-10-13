/** Which column a table is ordered by, and which way. */
export interface Sort {
	key: string;
	descending: boolean;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/**
 * The rows ordered by a value of each. Text is compared without regard to case and with numbers
 * inside it by value; rows without a value come last either way; equal rows keep their order.
 */
export function sortRows<Row>(
	rows: Row[],
	value: (row: Row) => string | number | null | undefined,
	descending: boolean
): Row[] {
	const direction = descending ? -1 : 1;
	return rows
		.map((row, index) => ({ row, index, value: value(row) }))
		.sort((a, b) => {
			const [x, y] = [a.value, b.value];
			if (x == null || y == null) return x == null && y == null ? a.index - b.index : x == null ? 1 : -1;
			const order = typeof x === 'number' && typeof y === 'number' ? x - y : collator.compare(String(x), String(y));
			return order === 0 ? a.index - b.index : order * direction;
		})
		.map((entry) => entry.row);
}

/** The rows whose text contains every word of the search, in any case. */
export function searchRows<Row>(rows: Row[], text: (row: Row) => string, query: string): Row[] {
	const words = query.toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return rows;
	return rows.filter((row) => {
		const haystack = text(row).toLowerCase();
		return words.every((word) => haystack.includes(word));
	});
}

/** How many pages a number of rows fills. Nothing still has one, empty, page. */
export function pageCount(total: number, size: number): number {
	return Math.max(1, Math.ceil(total / size));
}

/** The rows of a page, counted from 1. A page beyond the last one gives the last one. */
export function pageRows<Row>(rows: Row[], page: number, size: number): Row[] {
	const at = Math.min(Math.max(page, 1), pageCount(rows.length, size));
	return rows.slice((at - 1) * size, at * size);
}

/** Which rows a page shows: "26–50 of 420". */
export function rangeWords(page: number, size: number, total: number): string {
	if (total === 0) return '0 of 0';
	const first = (page - 1) * size + 1;
	return `${first}–${Math.min(page * size, total)} of ${total}`;
}

/** The ordering after a click on a column's heading: its natural direction first, the other on a second click. */
export function nextSort(current: Sort | null, key: string, descendingFirst: boolean): Sort {
	if (current?.key === key) return { key, descending: !current.descending };
	return { key, descending: descendingFirst };
}
