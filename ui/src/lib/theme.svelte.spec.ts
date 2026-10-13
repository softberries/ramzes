import { afterEach, describe, expect, it } from 'vitest';
import { ThemeState, initialTheme } from './theme.svelte';

afterEach(() => {
	localStorage.clear();
	document.documentElement.removeAttribute('data-theme');
});

describe('initialTheme', () => {
	it('prefers the saved choice', () => {
		expect(initialTheme('light', false)).toBe('light');
		expect(initialTheme('dark', true)).toBe('dark');
	});

	it('follows the system when nothing valid is saved', () => {
		expect(initialTheme(null, true)).toBe('light');
		expect(initialTheme(null, false)).toBe('dark');
		expect(initialTheme('purple', false)).toBe('dark');
	});
});

describe('ThemeState', () => {
	it('applies the saved theme to the document', () => {
		localStorage.setItem('ramzes-theme', 'light');
		const theme = new ThemeState();
		theme.init();
		expect(theme.current).toBe('light');
		expect(document.documentElement.getAttribute('data-theme')).toBe('light');
	});

	it('toggles, applies and remembers', () => {
		localStorage.setItem('ramzes-theme', 'dark');
		const theme = new ThemeState();
		theme.init();
		theme.toggle();
		expect(theme.current).toBe('light');
		expect(document.documentElement.getAttribute('data-theme')).toBe('light');
		expect(localStorage.getItem('ramzes-theme')).toBe('light');
		theme.toggle();
		expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
	});
});
