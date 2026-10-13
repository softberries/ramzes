/** Site-internal path with the configured base (`/ramzes/`), e.g. `href('install/helm/')`. */
export function href(path: string): string {
	const base = import.meta.env.BASE_URL.endsWith('/') ? import.meta.env.BASE_URL : `${import.meta.env.BASE_URL}/`;
	return base + path.replace(/^\//, '');
}

export const GITHUB_URL = 'https://github.com/softberries/ramzes';
export const CONTACT_EMAIL = 'hello@softberries.dev';
