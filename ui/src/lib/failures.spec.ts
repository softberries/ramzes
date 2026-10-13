import { describe, expect, it } from 'vitest';
import type { FailureRow } from './api/types.ts';
import { errorOf, groupByError } from './failures.ts';

function failure(id: string, last_error: string | null): FailureRow {
	return { id, last_error } as FailureRow;
}

describe('errorOf', () => {
	it('gives the error of a delivery, or says that there is none', () => {
		expect(errorOf(failure('1', 'HTTP 500'))).toBe('HTTP 500');
		expect(errorOf(failure('1', null))).toBe('No error was recorded');
		expect(errorOf({})).toBe('No error was recorded');
	});
});

describe('groupByError', () => {
	it('groups failures by their error, the largest group first', () => {
		const groups = groupByError([
			failure('1', 'HTTP 401'),
			failure('2', 'HTTP 500'),
			failure('3', null),
			failure('4', 'HTTP 500')
		]);
		expect(groups.map((group) => [group.error, group.rows.map((row) => row.id)])).toEqual([
			['HTTP 500', ['2', '4']],
			['HTTP 401', ['1']],
			['No error was recorded', ['3']]
		]);
	});

	it('keeps groups of equal size in the order their first failure appeared', () => {
		const groups = groupByError([failure('1', 'b'), failure('2', 'a')]);
		expect(groups.map((group) => group.error)).toEqual(['b', 'a']);
	});
});
