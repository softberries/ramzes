import { describe, expect, it } from 'vitest';
import { nextSort, pageCount, pageRows, rangeWords, searchRows, sortRows } from './table.ts';

const rows = [
	{ name: 'ops', sent: 12, last: 300 as number | null },
	{ name: 'Audit', sent: 2, last: null },
	{ name: 'on-call 10', sent: 12, last: 100 },
	{ name: 'on-call 9', sent: 40, last: 200 }
];

describe('sortRows', () => {
	it('orders text without regard to case, and numbers inside text by value', () => {
		expect(sortRows(rows, (r) => r.name, false).map((r) => r.name)).toEqual(['Audit', 'on-call 9', 'on-call 10', 'ops']);
	});

	it('orders numbers, keeping the given order of equal ones in both directions', () => {
		expect(sortRows(rows, (r) => r.sent, false).map((r) => r.name)).toEqual(['Audit', 'ops', 'on-call 10', 'on-call 9']);
		expect(sortRows(rows, (r) => r.sent, true).map((r) => r.name)).toEqual(['on-call 9', 'ops', 'on-call 10', 'Audit']);
	});

	it('puts rows without a value last, whichever way it is ordered', () => {
		expect(sortRows(rows, (r) => r.last, false).map((r) => r.last)).toEqual([100, 200, 300, null]);
		expect(sortRows(rows, (r) => r.last, true).map((r) => r.last)).toEqual([300, 200, 100, null]);
	});

	it('leaves the rows it was given untouched', () => {
		sortRows(rows, (r) => r.sent, true);
		expect(rows[0].name).toBe('ops');
	});
});

describe('searchRows', () => {
	const text = (r: (typeof rows)[number]) => `${r.name} ${r.sent}`;

	it('keeps rows that contain every word, in any case', () => {
		expect(searchRows(rows, text, 'ON-call 12').map((r) => r.name)).toEqual(['on-call 10']);
	});

	it('keeps everything for an empty search', () => {
		expect(searchRows(rows, text, '   ')).toHaveLength(4);
	});
});

describe('paging', () => {
	it('counts pages, with one page for nothing', () => {
		expect([pageCount(0, 25), pageCount(25, 25), pageCount(26, 25)]).toEqual([1, 1, 2]);
	});

	it('cuts the rows of a page and stays on the last page when asked for one beyond it', () => {
		expect(pageRows(rows, 2, 2).map((r) => r.name)).toEqual(['on-call 10', 'on-call 9']);
		expect(pageRows(rows, 9, 3).map((r) => r.name)).toEqual(['on-call 9']);
	});

	it('says which rows are shown', () => {
		expect(rangeWords(1, 25, 420)).toBe('1–25 of 420');
		expect(rangeWords(17, 25, 420)).toBe('401–420 of 420');
		expect(rangeWords(1, 25, 0)).toBe('0 of 0');
	});
});

describe('nextSort', () => {
	it('starts a column in its natural direction and flips it on the second click', () => {
		expect(nextSort(null, 'name', false)).toEqual({ key: 'name', descending: false });
		expect(nextSort({ key: 'name', descending: false }, 'name', false)).toEqual({ key: 'name', descending: true });
		expect(nextSort({ key: 'name', descending: true }, 'sent', true)).toEqual({ key: 'sent', descending: true });
	});
});
