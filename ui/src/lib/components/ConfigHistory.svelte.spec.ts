import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';

const { apiMock } = vi.hoisted(() => ({ apiMock: vi.fn() }));
vi.mock('#lib/api/client.ts', async (original) => ({
	...(await original<typeof import('#lib/api/client.ts')>()),
	api: apiMock
}));

import ConfigHistory from './ConfigHistory.svelte';

const NOW = Date.now();
const REVISIONS = [
	{ id: 'r2', summary: 'changed route everything', author: 'kris@example.com', created_at: NOW - 60_000 },
	{ id: 'r1', summary: 'added route everything', author: 'kris@example.com', created_at: NOW - 120_000 }
];
const CONFIGS: Record<string, string> = {
	r1: 'routes:\n- name: everything\n  stop: false\n',
	r2: 'routes:\n- name: everything\n  stop: true\n'
};

beforeEach(() => {
	apiMock.mockReset();
	apiMock.mockImplementation(async (method: string, path: string) => {
		if (method === 'POST') return undefined;
		if (path === '/api/config/revisions') return REVISIONS;
		return { config: CONFIGS[path.split('/').at(-1)!] };
	});
});

const kinds = async () =>
	[...(await page.getByTestId('config-diff').element()).querySelectorAll('[data-kind]')].map(
		(line) => `${line.getAttribute('data-kind')}:${line.textContent}`
	);

describe('ConfigHistory', () => {
	it('lists the changes with who made them', async () => {
		render(ConfigHistory);
		await expect.element(page.getByText('changed route everything')).toBeVisible();
		await expect.element(page.getByText('added route everything')).toBeVisible();
		await expect.element(page.getByText('kris@example.com').first()).toBeVisible();
	});

	it('shows what a change did against the configuration before it', async () => {
		render(ConfigHistory);
		await page.getByRole('button', { name: 'Show changes' }).first().click();
		await expect.element(page.getByTestId('config-diff')).toBeVisible();
		expect(await kinds()).toEqual([
			'same:  routes:',
			'same:  - name: everything',
			'removed:-   stop: false',
			'added:+   stop: true'
		]);
		await expect.element(page.getByText('This is the current configuration.')).toBeVisible();
	});

	it('shows the first change as all added and rolls back to it after confirmation', async () => {
		render(ConfigHistory);
		await page.getByRole('button', { name: 'Show changes' }).nth(1).click();
		await expect.element(page.getByTestId('config-diff')).toBeVisible();
		expect((await kinds()).every((line) => line.startsWith('added:'))).toBe(true);

		await page.getByRole('button', { name: 'Roll back to this' }).click();
		await expect.element(page.getByText('Source keys are not touched.')).toBeVisible();
		await page.getByRole('button', { name: 'Roll back', exact: true }).click();
		await vi.waitFor(() =>
			expect(apiMock).toHaveBeenCalledWith('POST', '/api/config/revisions/r1/rollback', {})
		);
	});
});
