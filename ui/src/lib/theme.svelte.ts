export type Theme = 'dark' | 'light';

const STORAGE_KEY = 'ramzes-theme';

/** The saved choice when there is one, otherwise what the system prefers. Dark is the default. */
export function initialTheme(saved: string | null, systemPrefersLight: boolean): Theme {
	if (saved === 'dark' || saved === 'light') return saved;
	return systemPrefersLight ? 'light' : 'dark';
}

export class ThemeState {
	current = $state<Theme>('dark');

	/** Reads the saved or system theme and applies it to the document. */
	init() {
		const prefersLight = window.matchMedia('(prefers-color-scheme: light)').matches;
		this.current = initialTheme(localStorage.getItem(STORAGE_KEY), prefersLight);
		this.apply();
	}

	toggle() {
		this.current = this.current === 'dark' ? 'light' : 'dark';
		localStorage.setItem(STORAGE_KEY, this.current);
		this.apply();
	}

	private apply() {
		document.documentElement.setAttribute('data-theme', this.current);
	}
}

export const theme = new ThemeState();
