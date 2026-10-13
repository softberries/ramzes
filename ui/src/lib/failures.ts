import type { FailureRow } from './api/types.ts';

export interface FailureGroup {
	error: string;
	rows: FailureRow[];
}

/** Failures grouped by their error text, the largest group first. */
export function groupByError(rows: FailureRow[]): FailureGroup[] {
	const groups: FailureGroup[] = [];
	for (const row of rows) {
		const error = row.last_error ?? 'No error was recorded';
		const group = groups.find((g) => g.error === error);
		if (group) group.rows.push(row);
		else groups.push({ error, rows: [row] });
	}
	// A stable sort keeps groups of equal size in the order their first failure appeared.
	return groups.sort((a, b) => b.rows.length - a.rows.length);
}
