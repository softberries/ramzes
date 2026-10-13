import { redirect } from '@sveltejs/kit';
import { ApiError, api } from '#lib/api/client.ts';
import type { Me } from '#lib/api/types.ts';
import type { LayoutLoad } from './$types';

// The UI is a single-page app served as static files by the ramzes binary.
export const ssr = false;
export const prerender = false;

/** Sends people to first-run setup or to sign in when that is what the instance needs. */
export const load: LayoutLoad = async ({ url }) => {
	const path = url.pathname;
	const { needs_setup } = await api<{ needs_setup: boolean }>('GET', '/api/setup');
	if (needs_setup) {
		if (path !== '/setup') redirect(307, '/setup');
		return { me: null as Me | null };
	}
	try {
		const me = await api<Me>('GET', '/api/auth/me');
		if (path === '/login') redirect(307, '/');
		return { me: me as Me | null };
	} catch (error) {
		if (error instanceof ApiError && error.status === 401) {
			if (path !== '/login') redirect(307, '/login');
			return { me: null as Me | null };
		}
		throw error;
	}
};
