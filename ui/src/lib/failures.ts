import type { FailureRow } from './api/types.ts';

export interface FailureGroup {
	error: string;
	rows: FailureRow[];
}

/** What went wrong with a delivery, in the words it is listed and grouped by. */
export function errorOf(row: Pick<FailureRow, 'last_error'>): string {
	return row.last_error ?? 'No error was recorded';
}

/** Failures grouped by their error text, the largest group first. */
export function groupByError(rows: FailureRow[]): FailureGroup[] {
	const groups: FailureGroup[] = [];
	for (const row of rows) {
		const error = errorOf(row);
		const group = groups.find((g) => g.error === error);
		if (group) group.rows.push(row);
		else groups.push({ error, rows: [row] });
	}
	// A stable sort keeps groups of equal size in the order their first failure appeared.
	return groups.sort((a, b) => b.rows.length - a.rows.length);
}
