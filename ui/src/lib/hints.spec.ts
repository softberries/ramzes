import { describe, expect, it } from 'vitest';
import { dismiss, isDismissed, lacksPublicUrl } from './hints.ts';

function storage(): Pick<Storage, 'getItem' | 'setItem'> & { kept: Record<string, string> } {
	const kept: Record<string, string> = {};
	return { kept, getItem: (key) => kept[key] ?? null, setItem: (key, value) => void (kept[key] = value) };
}

describe('dismissed hints', () => {
	it('shows a hint until it is dismissed, and remembers each hint on its own', () => {
		const store = storage();
		expect(isDismissed(store, 'no-public-url')).toBe(false);
		dismiss(store, 'no-public-url');
		expect(isDismissed(store, 'no-public-url')).toBe(true);
		expect(isDismissed(store, 'another')).toBe(false);
		expect(Object.keys(store.kept)).toEqual(['ramzes-hint-no-public-url']);
	});

	it('keeps working when the browser refuses storage', () => {
		const refusing = {
			getItem: () => {
				throw new Error('denied');
			},
			setItem: () => {
				throw new Error('denied');
			}
		};
		expect(isDismissed(refusing, 'no-public-url')).toBe(false);
		expect(() => dismiss(refusing, 'no-public-url')).not.toThrow();
	});
});

describe('lacksPublicUrl', () => {
	it('is true only for loaded settings without an address', () => {
		expect(lacksPublicUrl({ public_url: null })).toBe(true);
		expect(lacksPublicUrl({})).toBe(true);
		expect(lacksPublicUrl({ public_url: '  ' })).toBe(true);
		expect(lacksPublicUrl({ public_url: 'https://ramzes.example.com' })).toBe(false);
		// Not loaded yet: nothing is known, so nothing is claimed.
		expect(lacksPublicUrl(null)).toBe(false);
	});
});
