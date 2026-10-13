import { describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import Palette from './Palette.svelte';
import SeverityMark from './SeverityMark.svelte';
import StateTag from './StateTag.svelte';

describe('SeverityMark', () => {
	it('uses the severity token and names the severity', async () => {
		const { container } = render(SeverityMark, { severity: 'critical' });
		const mark = container.querySelector('[data-severity="critical"]') as HTMLElement;
		expect(mark.style.color).toBe('var(--sev-critical)');
		await expect.element(page.getByText('critical')).toBeInTheDocument();
	});
});

describe('StateTag', () => {
	it.each([
		['delivered', 'delivered'],
		['in_progress', 'sending'],
		['dead', 'gave up'],
		['unrouted', 'no route']
	] as const)('shows %s as "%s" in its own colour', async (state, words) => {
		const { container } = render(StateTag, { state });
		const tag = container.querySelector(`[data-state="${state}"]`) as HTMLElement;
		expect(tag.style.color).toBe(`var(--st-${state})`);
		expect(tag.textContent).toBe(words);
	});
});

describe('Palette', () => {
	it('filters pages and goes to the selected one on Enter', async () => {
		const onnavigate = vi.fn();
		render(Palette, { open: true, onnavigate });
		const input = page.getByRole('textbox', { name: 'Go to a page' });
		await input.fill('con');
		await expect.element(page.getByRole('button', { name: 'Connections' })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Feed' })).not.toBeInTheDocument();
		await userEvent.keyboard('{Enter}');
		expect(onnavigate).toHaveBeenCalledWith('/connections');
	});

	it('moves the selection with the arrow keys', async () => {
		const onnavigate = vi.fn();
		render(Palette, { open: true, onnavigate });
		await page.getByRole('textbox', { name: 'Go to a page' }).click();
		await userEvent.keyboard('{ArrowDown}{ArrowDown}{Enter}');
		expect(onnavigate).toHaveBeenCalledWith('/notifications');
	});

	it('says so when nothing matches', async () => {
		render(Palette, { open: true, onnavigate: vi.fn() });
		await page.getByRole('textbox', { name: 'Go to a page' }).fill('zzz');
		await expect.element(page.getByText('No page matches “zzz”.')).toBeInTheDocument();
	});
});
