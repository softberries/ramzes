/** The top-level pages in navigation order, grouped as the navigation shows them. */
export const NAV_GROUPS = [
	{
		title: 'Happening now',
		pages: [
			{ href: '/overview', label: 'Overview' },
			{ href: '/', label: 'Feed' },
			{ href: '/notifications', label: 'Notifications' },
			{ href: '/failures', label: 'Failures' },
			{ href: '/silences', label: 'Silences' }
		]
	},
	{
		title: 'Configuration',
		pages: [
			{ href: '/routes', label: 'Routes' },
			{ href: '/templates', label: 'Templates' },
			{ href: '/connections', label: 'Connections' },
			{ href: '/sources', label: 'Sources' },
			{ href: '/settings', label: 'Settings' }
		]
	}
] as const;

export const PAGES = NAV_GROUPS.flatMap((group) => [...group.pages]);

export type NavPage = (typeof PAGES)[number];
export type PageHref = NavPage['href'];

/** The page a path belongs to, or null outside the navigation. An event's own page belongs to the feed. */
export function currentPage(path: string): NavPage | null {
	if (path === '/' || path.startsWith('/events/')) return PAGES.find((page) => page.href === '/') ?? null;
	return PAGES.find((page) => page.href !== '/' && (path === page.href || path.startsWith(`${page.href}/`))) ?? null;
}
