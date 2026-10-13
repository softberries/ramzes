import { describe, expect, it } from 'vitest';
import type { FailureRow, Route, RouteInput, RouteResult } from './api/types';
import { groupByError } from './failures';
import {
	SAMPLE_EVENT,
	cleanMatch,
	cleanTargets,
	inputOfRoute,
	moved,
	outcomeWords,
	parseSampleEvent,
	routesForTest,
	routesUsing,
	summarize
} from './routes';

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

	it('ends with the expression', () => {
		expect(summarize({ expr: 'payload.amount > 100' }, sources)).toBe('where payload.amount > 100');
		expect(summarize({ topic: 'orders.*', expr: "labels.env == 'prod'" }, sources)).toBe(
			"topic orders.*, where labels.env == 'prod'"
		);
		expect(summarize({ expr: null }, sources)).toBe('Every event');
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

	it('keeps an expression, trimmed, and drops a blank one', () => {
		expect(cleanMatch({ expr: '  payload.amount > 100 ' })).toEqual({ expr: 'payload.amount > 100' });
		expect(cleanMatch({ expr: '   ' })).toEqual({});
		expect(cleanMatch({ expr: null })).toEqual({});
	});
});

describe('cleanTargets', () => {
	const takes = (id: string) => id === 'mail';

	it('drops rows without a connection and empty optional values', () => {
		expect(
			cleanTargets(
				[
					{ connection_id: '' },
					{ connection_id: 'hook', destination: '', template_id: '' },
					{ connection_id: 'mail', destination: ' ops@example.com ', template_id: 't1' }
				],
				takes
			)
		).toEqual([{ connection_id: 'hook' }, { connection_id: 'mail', destination: 'ops@example.com', template_id: 't1' }]);
	});

	it('drops a destination the connector does not take', () => {
		expect(cleanTargets([{ connection_id: 'hook', destination: '#ops' }], takes)).toEqual([{ connection_id: 'hook' }]);
	});
});

describe('routesForTest', () => {
	const stored = [route('1', ['c1']), { ...route('2', ['c2']), stop: true, enabled: false }];
	const input: RouteInput = { name: 'draft', enabled: true, stop: false, match: { expr: 'true' }, targets: [] };

	it('sends the stored routes in order when nothing is being edited', () => {
		expect(routesForTest(stored, null)).toEqual([
			{ name: 'route 1', enabled: true, stop: false, match: {}, targets: [{ connection_id: 'c1' }] },
			{ name: 'route 2', enabled: false, stop: true, match: {}, targets: [{ connection_id: 'c2' }] }
		]);
	});

	it("keeps a stored route's shaping policy", () => {
		const shaped = { ...route('3', ['c1']), shaping: { group: { by: ['service'], wait_s: 30 } } };
		expect(routesForTest([shaped], null)[0].shaping).toEqual({ group: { by: ['service'], wait_s: 30 } });
		expect(inputOfRoute(shaped)).toEqual({
			name: 'route 3',
			enabled: true,
			stop: false,
			match: {},
			targets: [{ connection_id: 'c1' }],
			shaping: { group: { by: ['service'], wait_s: 30 } }
		});
	});

	it("keeps a stored route's escalation chain", () => {
		const escalation = [{ after_s: 300, targets: [{ connection_id: 'c2', destination: 'oncall' }] }];
		const paged = { ...route('4', ['c1']), escalation };
		expect(inputOfRoute(paged).escalation).toEqual(escalation);
		expect(routesForTest([paged], null)[0].escalation).toEqual(escalation);
		expect('escalation' in inputOfRoute(route('5', ['c1']))).toBe(false);
	});

	it('puts the draft in place of the route being edited', () => {
		const sent = routesForTest(stored, { id: '2', input });
		expect(sent.map((r) => r.name)).toEqual(['route 1', 'draft']);
		expect(sent[1]).toEqual(input);
	});

	it('appends a new draft', () => {
		expect(routesForTest(stored, { id: null, input }).map((r) => r.name)).toEqual(['route 1', 'route 2', 'draft']);
	});

	it('names a draft that has no name yet', () => {
		const sent = routesForTest([], { id: null, input: { ...input, name: '  ' } });
		expect(sent[0].name).toBe('New route');
	});
});

describe('parseSampleEvent', () => {
	it('reads the prefilled sample', () => {
		const parsed = parseSampleEvent(SAMPLE_EVENT);
		expect(parsed.problem).toBeUndefined();
		expect(parsed.event).toMatchObject({ topic: 'orders.payment.failed', severity: 'error' });
	});

	it('says what is wrong with text that is not an event', () => {
		expect(parseSampleEvent('{').problem).toBe('The sample event is not valid JSON.');
		expect(parseSampleEvent('[1]').problem).toBe('The sample event must be a JSON object with a topic and a title.');
		expect(parseSampleEvent('{"topic": "a"}').problem).toBe(
			'The sample event must be a JSON object with a topic and a title.'
		);
	});
});

describe('outcomeWords', () => {
	const result = (outcome: string, error: string | null = null): RouteResult => ({
		name: 'r',
		outcome,
		error,
		targets: []
	});

	it('words each outcome', () => {
		expect(outcomeWords(result('matched'))).toBe('matched');
		expect(outcomeWords(result('not_matched'))).toBe('not matched');
		expect(outcomeWords(result('disabled'))).toBe('off');
		expect(outcomeWords(result('skipped'))).toBe('skipped, an earlier route stops here');
		expect(outcomeWords(result('error', 'unknown filter nosuch'))).toBe('expression failed: unknown filter nosuch');
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
