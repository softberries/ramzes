import { describe, expect, it } from 'vitest';
import type { FailureRow, Route } from './api/types';
import { groupByError } from './failures';
import { cleanMatch, moved, routesUsing, summarize } from './routes';

function route(id: string, connections: string[]): Route {
	return {
		id,
		name: `route ${id}`,
		position: 1,
		enabled: true,
		stop: false,
		match: {},
		targets: connections.map((connection_id) => ({ connection_id }))
	};
}

describe('summarize', () => {
	const sources = { s1: 'orders-api' };

	it('describes a route that matches everything', () => {
		expect(summarize({}, sources)).toBe('Every event');
		expect(summarize({ labels: [], topic: null }, sources)).toBe('Every event');
	});

	it('joins each condition in plain words', () => {
		const text = summarize(
			{
				topic: 'orders.*',
				min_severity: 'error',
				source_id: 's1',
				labels: [
					{ op: 'equals', key: 'env', value: 'prod' },
					{ op: 'matches', key: 'service', pattern: '^pay' },
					{ op: 'exists', key: 'team' }
				]
			},
			sources
		);
		expect(text).toBe(
			'topic orders.*, severity error or higher, from orders-api, env = prod, service matches ^pay, has label team'
		);
	});

	it('says critical only for the highest severity and names an unknown source', () => {
		expect(summarize({ min_severity: 'critical' }, sources)).toBe('severity critical');
		expect(summarize({ source_id: 'gone' }, sources)).toBe('from a deleted source');
	});
});

describe('cleanMatch', () => {
	it('drops empty values and incomplete label conditions', () => {
		expect(
			cleanMatch({
				topic: '  ',
				min_severity: null,
				source_id: '',
				labels: [
					{ op: 'equals', key: '', value: 'x' },
					{ op: 'equals', key: 'env', value: 'prod' },
					{ op: 'matches', key: 'service', pattern: '' },
					{ op: 'exists', key: ' team ' }
				]
			})
		).toEqual({
			labels: [
				{ op: 'equals', key: 'env', value: 'prod' },
				{ op: 'exists', key: 'team' }
			]
		});
	});

	it('keeps set values, trimmed', () => {
		expect(cleanMatch({ topic: ' orders.* ', min_severity: 'warning', source_id: 's1', labels: [] })).toEqual({
			topic: 'orders.*',
			min_severity: 'warning',
			source_id: 's1'
		});
	});
});

describe('moved', () => {
	const ids = ['a', 'b', 'c'];

	it('moves an id one place up or down', () => {
		expect(moved(ids, 'b', -1)).toEqual(['b', 'a', 'c']);
		expect(moved(ids, 'b', 1)).toEqual(['a', 'c', 'b']);
	});

	it('returns null when the move is not possible', () => {
		expect(moved(ids, 'a', -1)).toBeNull();
		expect(moved(ids, 'c', 1)).toBeNull();
		expect(moved(ids, 'zzz', 1)).toBeNull();
	});
});

describe('routesUsing', () => {
	it('finds the routes that target a connection', () => {
		const routes = [route('1', ['c1']), route('2', ['c2']), route('3', ['c2', 'c1'])];
		expect(routesUsing(routes, 'c1').map((r) => r.id)).toEqual(['1', '3']);
		expect(routesUsing(routes, 'c9')).toEqual([]);
	});
});

describe('groupByError', () => {
	function failure(id: string, error: string | null): FailureRow {
		return { id, last_error: error, event_id: `e-${id}`, event_title: 't' } as FailureRow;
	}

	it('groups failures with the same error, largest group first', () => {
		const groups = groupByError([
			failure('1', 'HTTP 401: invalid_token'),
			failure('2', 'the request timed out'),
			failure('3', 'HTTP 401: invalid_token'),
			failure('4', null)
		]);
		expect(groups.map((g) => [g.error, g.rows.map((r) => r.id)])).toEqual([
			['HTTP 401: invalid_token', ['1', '3']],
			['the request timed out', ['2']],
			['No error was recorded', ['4']]
		]);
	});

	it('is empty for no failures', () => {
		expect(groupByError([])).toEqual([]);
	});
});
