import { describe, expect, it } from 'vitest';
import { NAV_GROUPS, PAGES, currentPage } from './pages.ts';

describe('PAGES', () => {
	it('lists every page once, in navigation order', () => {
		expect(PAGES.map((p) => p.label)).toEqual([
			'Overview',
			'Feed',
			'Notifications',
			'Failures',
			'Silences',
			'Routes',
			'Templates',
			'Connections',
			'Sources',
			'Settings'
		]);
	});

	it('is the groups laid end to end', () => {
		expect(NAV_GROUPS.flatMap((group) => [...group.pages])).toEqual([...PAGES]);
		expect(NAV_GROUPS.map((group) => group.title)).toEqual(['Happening now', 'Configuration']);
	});
});

describe('currentPage', () => {
	it('finds the page a path belongs to', () => {
		expect(currentPage('/')?.label).toBe('Feed');
		expect(currentPage('/overview')?.label).toBe('Overview');
		expect(currentPage('/notifications')?.label).toBe('Notifications');
		expect(currentPage('/settings')?.label).toBe('Settings');
	});

	it('counts an event page as the feed and a notification page as notifications', () => {
		expect(currentPage('/events/01J')?.label).toBe('Feed');
		expect(currentPage('/notifications/01J')?.label).toBe('Notifications');
	});

	it('knows no page for a path outside the navigation', () => {
		expect(currentPage('/login')).toBeNull();
		expect(currentPage('/routesque')).toBeNull();
	});
});
