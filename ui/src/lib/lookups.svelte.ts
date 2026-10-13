import { api } from './api/client.ts';
import type { ConnectionView, Source } from './api/types.ts';

/** Sources and connections, loaded once per page that needs their names. */
class Lookups {
	sources = $state<Source[]>([]);
	connections = $state<ConnectionView[]>([]);

	sourceNames = $derived(Object.fromEntries(this.sources.map((s) => [s.id, s.name])));
	connectionNames = $derived(Object.fromEntries(this.connections.map((c) => [c.id, c.name])));

	async load() {
		[this.sources, this.connections] = await Promise.all([
			api<Source[]>('GET', '/api/sources'),
			api<ConnectionView[]>('GET', '/api/connections')
		]);
	}
}

export const lookups = new Lookups();
