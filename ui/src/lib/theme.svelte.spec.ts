import { afterEach, describe, expect, it } from 'vitest';
import { ThemeState, initialTheme } from './theme.svelte';

afterEach(() => {
	localStorage.clear();
	document.documentElement.removeAttribute('data-theme');
});

describe('initialTheme', () => {
	it('uses the saved choice', () => {
		expect(initialTheme('light')).toBe('light');
		expect(initialTheme('dark')).toBe('dark');
	});

	it('is dark when nothing valid is saved', () => {
		expect(initialTheme(null)).toBe('dark');
		expect(initialTheme('purple')).toBe('dark');
	});
});

describe('ThemeState', () => {
	it('starts dark on a first visit', () => {
		const theme = new ThemeState();
		theme.init();
		expect(theme.current).toBe('dark');
		expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
	});

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
