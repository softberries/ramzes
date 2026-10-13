import { describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { RouteMatch } from '#lib/api/types.ts';
import MatchEditorHarness from './MatchEditorHarness.svelte';

const SOURCES = [{ id: 's1', name: 'orders-api' }];

/** Renders the editor and returns a reader for the match it currently holds. */
function editing(initial: RouteMatch = {}) {
	render(MatchEditorHarness, { initial, sources: SOURCES });
	return async () => JSON.parse((await page.getByTestId('match').element()).textContent ?? '{}') as RouteMatch;
}

describe('MatchEditor', () => {
	it('starts as a match for every event', async () => {
		const match = editing();
		expect(await match()).toEqual({});
		await expect.element(page.getByTestId('summary')).toHaveTextContent('Every event');
	});

	it('builds topic, severity and source conditions', async () => {
		const match = editing();
		await page.getByLabelText('Topic').fill('orders.*');
		await page.getByLabelText('Minimum severity').selectOptions('error');
		await page.getByLabelText('Source').selectOptions('s1');
		expect(await match()).toEqual({ topic: 'orders.*', min_severity: 'error', source_id: 's1' });
		await expect
			.element(page.getByTestId('summary'))
			.toHaveTextContent('topic orders.*, severity error or higher, from orders-api');
	});

	it('builds each kind of label condition', async () => {
		const match = editing();
		const add = page.getByRole('button', { name: 'Add label condition' });

		await add.click();
		await page.getByLabelText('Label key').nth(0).fill('env');
		await page.getByLabelText('Label value').nth(0).fill('prod');

		await add.click();
		await page.getByLabelText('Condition', { exact: true }).nth(1).selectOptions('matches');
		await page.getByLabelText('Label key').nth(1).fill('service');
		await page.getByLabelText('Pattern').fill('^pay');

		await add.click();
		await page.getByLabelText('Condition', { exact: true }).nth(2).selectOptions('exists');
		await page.getByLabelText('Label key').nth(2).fill('team');

		expect(await match()).toEqual({
			labels: [
				{ op: 'equals', key: 'env', value: 'prod' },
				{ op: 'matches', key: 'service', pattern: '^pay' },
				{ op: 'exists', key: 'team' }
			]
		});
	});

	it('adds an expression and drops it when cleared', async () => {
		const match = editing();
		await page.getByLabelText('Expression').fill("payload.amount > 100 and labels.env == 'prod'");
		expect(await match()).toEqual({ expr: "payload.amount > 100 and labels.env == 'prod'" });
		await expect
			.element(page.getByTestId('summary'))
			.toHaveTextContent("where payload.amount > 100 and labels.env == 'prod'");
		await page.getByLabelText('Expression').fill('');
		expect(await match()).toEqual({});
	});

	it('removes a label condition', async () => {
		const match = editing({ labels: [{ op: 'equals', key: 'env', value: 'prod' }] });
		await expect.element(page.getByLabelText('Label key')).toHaveValue('env');
		await page.getByRole('button', { name: 'Remove condition' }).click();
		expect(await match()).toEqual({});
	});
});
