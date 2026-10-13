/** The top-level pages in navigation order, grouped as the navigation shows them. */
export const NAV_GROUPS = [
	{
		title: 'Happening now',
		pages: [
			{ href: '/overview', label: 'Overview', icon: 'overview' },
			{ href: '/', label: 'Feed', icon: 'feed' },
			{ href: '/notifications', label: 'Notifications', icon: 'bell' },
			{ href: '/failures', label: 'Failures', icon: 'alert' },
			{ href: '/silences', label: 'Silences', icon: 'mute' }
		]
	},
	{
		title: 'Configuration',
		pages: [
			{ href: '/routes', label: 'Routes', icon: 'route' },
			{ href: '/templates', label: 'Templates', icon: 'template' },
			{ href: '/connections', label: 'Connections', icon: 'plug' },
			{ href: '/sources', label: 'Sources', icon: 'inbox' },
			{ href: '/settings', label: 'Settings', icon: 'settings' }
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
