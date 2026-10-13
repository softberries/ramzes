export type Theme = 'dark' | 'light';

const STORAGE_KEY = 'ramzes-theme';

/** The saved choice when there is one. Dark is the default; light is something people choose. */
export function initialTheme(saved: string | null): Theme {
	return saved === 'light' ? 'light' : 'dark';
}

export class ThemeState {
	current = $state<Theme>('dark');

	/** Reads the saved theme and applies it to the document. */
	init() {
		this.current = initialTheme(localStorage.getItem(STORAGE_KEY));
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
