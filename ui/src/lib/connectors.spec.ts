import { describe, expect, it } from 'vitest';
import type { ConnectorDescriptor } from './api/types';
import { groupConnectors, healthWords } from './connectors';

function connector(kind: string, custom = false): ConnectorDescriptor {
	return {
		kind,
		name: kind,
		description: '',
		custom,
		fields: [],
		capabilities: { rich: false, update: false, threads: false, buttons: false, max_len: 1000 }
	};
}

describe('groupConnectors', () => {
	it('groups connectors by what they are for, keeping the order within a group', () => {
		const groups = groupConnectors(
			['slack', 'webhook', 'email', 'discord', 'teams', 'ntfy', 'telegram', 'pushover'].map((kind) => connector(kind))
		);
		expect(groups.map((g) => [g.title, g.connectors.map((c) => c.kind)])).toEqual([
			['Chat', ['slack', 'discord', 'teams', 'telegram']],
			['Email and push', ['email', 'ntfy', 'pushover']],
			['Generic', ['webhook']]
		]);
	});

	it('puts custom connectors in their own group and unknown ones under generic', () => {
		const groups = groupConnectors([connector('pager'), connector('chat', true), connector('slack')]);
		expect(groups.map((g) => [g.title, g.connectors.map((c) => c.kind)])).toEqual([
			['Chat', ['slack']],
			['Generic', ['pager']],
			['Your own', ['chat']]
		]);
	});

	it('is empty for no connectors', () => {
		expect(groupConnectors([])).toEqual([]);
	});
});

describe('healthWords', () => {
	const health = (delivered: number, failed: number, pending = 0) => ({
		connection_id: 'c',
		delivered,
		failed,
		pending
	});

	it('counts what was delivered and what failed', () => {
		expect(healthWords(health(12, 1))).toBe('12 delivered, 1 failed');
		expect(healthWords(health(3, 0))).toBe('3 delivered, 0 failed');
	});

	it('adds what is still waiting', () => {
		expect(healthWords(health(3, 0, 2))).toBe('3 delivered, 0 failed, 2 waiting');
	});

	it('says so when nothing was sent', () => {
		expect(healthWords(health(0, 0))).toBe('nothing sent');
	});
});
