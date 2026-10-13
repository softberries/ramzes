import type { ConnectionHealth, ConnectorDescriptor } from './api/types.ts';

export interface ConnectorGroup {
	title: string;
	connectors: ConnectorDescriptor[];
}

const CHAT = ['slack', 'discord', 'teams', 'telegram'];
const EMAIL_AND_PUSH = ['email', 'ntfy', 'pushover'];

/** The connector catalogue grouped by what a connector is for. Groups with nothing in them are left out. */
export function groupConnectors(connectors: ConnectorDescriptor[]): ConnectorGroup[] {
	const shipped = connectors.filter((c) => !c.custom);
	const groups: ConnectorGroup[] = [
		{ title: 'Chat', connectors: shipped.filter((c) => CHAT.includes(c.kind)) },
		{ title: 'Email and push', connectors: shipped.filter((c) => EMAIL_AND_PUSH.includes(c.kind)) },
		{ title: 'Generic', connectors: shipped.filter((c) => !CHAT.includes(c.kind) && !EMAIL_AND_PUSH.includes(c.kind)) },
		{ title: 'Your own', connectors: connectors.filter((c) => c.custom) }
	];
	return groups.filter((group) => group.connectors.length > 0);
}

/** How a connection's deliveries went, in a few words. */
export function healthWords(health: ConnectionHealth): string {
	if (health.delivered + health.failed + health.pending === 0) return 'nothing sent';
	const waiting = health.pending > 0 ? `, ${health.pending} waiting` : '';
	return `${health.delivered} delivered, ${health.failed} failed${waiting}`;
}

/** A small connector definition that works as written, to start a new one from. */
export const EXAMPLE_DEFINITION = `kind: teamchat
name: Team chat
description: Posts each message to our own chat server.
fields:
  - { key: base_url, label: Base URL, kind: url, required: true }
  - { key: token, label: Token, secret: true, required: true }
request:
  url: "{{ settings.base_url }}/messages"
  headers:
    Authorization: "Bearer {{ credentials.token }}"
  body: '{"text": {{ message.title | tojson }}}'
`;
