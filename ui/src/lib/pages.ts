/** The top-level pages, in navigation order. */
export const PAGES = [
	{ href: '/', label: 'Feed' },
	{ href: '/failures', label: 'Failures' },
	{ href: '/routes', label: 'Routes' },
	{ href: '/templates', label: 'Templates' },
	{ href: '/connections', label: 'Connections' },
	{ href: '/sources', label: 'Sources' },
	{ href: '/settings', label: 'Settings' }
] as const;

export type PageHref = (typeof PAGES)[number]['href'];
