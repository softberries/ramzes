/**
 * Screenshot lookup: the images in `src/assets/screens`, written by `scripts/capture.mjs`, by base
 * name (`overview` for `overview-dark.webp` and `overview-light.webp`).
 */
import type { ImageMetadata } from 'astro';

const files = import.meta.glob<{ default: ImageMetadata }>('../assets/screens/*.webp', { eager: true });

/** Both theme variants of a screenshot; fails the build when either is missing. */
export function shot(name: string): { dark: ImageMetadata; light: ImageMetadata } {
	const get = (theme: 'dark' | 'light') => {
		const file = files[`../assets/screens/${name}-${theme}.webp`];
		if (!file) throw new Error(`No screenshot ${name}-${theme}.webp in src/assets/screens. Run npm run capture.`);
		return file.default;
	};
	return { dark: get('dark'), light: get('light') };
}
