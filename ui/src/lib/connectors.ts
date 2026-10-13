import type { ConnectionHealth, ConnectorDescriptor } from './api/types.ts';

export interface ConnectorGroup {
	title: string;
	connectors: ConnectorDescriptor[];
}

const CHAT = ['slack', 'slack_app', 'discord', 'teams', 'telegram'];
const PAGING = ['pagerduty', 'opsgenie'];
const EMAIL_AND_PUSH = ['email', 'ntfy', 'pushover'];
const GROUPED = [...CHAT, ...PAGING, ...EMAIL_AND_PUSH];

/** The connectors of one group, in the order the group names them. */
function pick(connectors: ConnectorDescriptor[], kinds: string[]): ConnectorDescriptor[] {
	return kinds.flatMap((kind) => connectors.filter((c) => c.kind === kind));
}

/** The connector catalogue grouped by what a connector is for. Groups with nothing in them are left out. */
export function groupConnectors(connectors: ConnectorDescriptor[]): ConnectorGroup[] {
	const shipped = connectors.filter((c) => !c.custom);
	const groups: ConnectorGroup[] = [
		{ title: 'Chat', connectors: pick(shipped, CHAT) },
		{ title: 'Paging', connectors: pick(shipped, PAGING) },
		{ title: 'Email and push', connectors: pick(shipped, EMAIL_AND_PUSH) },
		{ title: 'Generic', connectors: shipped.filter((c) => !GROUPED.includes(c.kind)) },
		{ title: 'Your own', connectors: connectors.filter((c) => c.custom) }
	];
	return groups.filter((group) => group.connectors.length > 0);
}

/** What a connector can do beyond sending a message, for the catalogue. Empty when it only sends. */
export function capabilityWords(capabilities: ConnectorDescriptor['capabilities']): string {
	const words: string[] = [];
	if (capabilities.buttons) words.push('buttons');
	if (capabilities.threads) words.push('threads');
	if (capabilities.update) words.push('updates its messages');
	return words.join(', ');
}

/** Where Slack must send the button presses of one Slack app connection. `origin` is this instance's address. */
export function interactionsUrl(origin: string, connectionId: string): string {
	return `${origin}/v1/slack/${connectionId}/interactions`;
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
