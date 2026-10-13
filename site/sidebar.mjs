// The documentation's table of contents. astro.config.mjs turns it into the sidebar; every slug is
// a page under src/content/docs.
export const sidebar = [
	{
		label: 'Getting started',
		items: [
			{ label: 'What is ramzes', slug: 'getting-started/what-is-ramzes' },
			{ label: 'Quickstart', slug: 'getting-started/quickstart' },
			{ label: 'Your first route', slug: 'getting-started/first-route' },
		],
	},
	{
		label: 'Install',
		items: [
			{ label: 'Docker Compose', slug: 'install/docker-compose' },
			{ label: 'Helm and Kubernetes', slug: 'install/helm' },
			{ label: 'From source', slug: 'install/from-source' },
			{ label: 'Upgrading', slug: 'install/upgrading' },
		],
	},
	{
		label: 'Concepts',
		items: [
			{ label: 'Architecture', slug: 'concepts/architecture' },
			{ label: 'Events', slug: 'concepts/events' },
			{ label: 'Routes', slug: 'concepts/routes' },
			{ label: 'Notifications', slug: 'concepts/notifications' },
			{ label: 'Noise control', slug: 'concepts/noise-control' },
			{ label: 'Escalation and acknowledgement', slug: 'concepts/escalation' },
			{ label: 'Delivery and retries', slug: 'concepts/delivery' },
			{ label: 'Templates', slug: 'concepts/templates' },
		],
	},
	{
		label: 'Sending events',
		collapsed: true,
		items: [
			{ label: 'The events API', slug: 'sending/events-api' },
			{ label: 'Alertmanager', slug: 'sending/alertmanager' },
			{ label: 'Grafana', slug: 'sending/grafana' },
			{ label: 'GitHub', slug: 'sending/github' },
			{ label: 'GitLab', slug: 'sending/gitlab' },
			{ label: 'Sentry', slug: 'sending/sentry' },
			{ label: 'Any JSON webhook', slug: 'sending/any-json' },
		],
	},
	{
		label: 'Channels',
		collapsed: true,
		items: [
			{ label: 'Overview', slug: 'channels/overview' },
			{ label: 'Slack', slug: 'channels/slack' },
			{ label: 'Microsoft Teams', slug: 'channels/teams' },
			{ label: 'Discord', slug: 'channels/discord' },
			{ label: 'Email', slug: 'channels/email' },
			{ label: 'ntfy, Telegram and Pushover', slug: 'channels/push' },
			{ label: 'PagerDuty and Opsgenie', slug: 'channels/paging' },
			{ label: 'Webhook', slug: 'channels/webhook' },
			{ label: 'Your own connector', slug: 'channels/custom' },
		],
	},
	{
		label: 'User guide',
		collapsed: true,
		items: [
			{ label: 'Tour of the app', slug: 'guide/tour' },
			{ label: 'Overview', slug: 'guide/overview' },
			{ label: 'Feed and event journey', slug: 'guide/feed' },
			{ label: 'Notifications', slug: 'guide/notifications' },
			{ label: 'Failures', slug: 'guide/failures' },
			{ label: 'Silences', slug: 'guide/silences' },
			{ label: 'Routes and the tester', slug: 'guide/routes' },
			{ label: 'Templates', slug: 'guide/templates' },
			{ label: 'Connections', slug: 'guide/connections' },
			{ label: 'Sources', slug: 'guide/sources' },
			{ label: 'Settings and history', slug: 'guide/settings' },
		],
	},
	{
		label: 'Operations',
		collapsed: true,
		items: [
			{ label: 'Configuration reference', slug: 'operations/configuration' },
			{ label: 'Security', slug: 'operations/security' },
			{ label: 'Backup, export and import', slug: 'operations/backup' },
			{ label: 'Metrics and health', slug: 'operations/metrics' },
			{ label: 'Retention', slug: 'operations/retention' },
			{ label: 'Troubleshooting', slug: 'operations/troubleshooting' },
		],
	},
	{
		label: 'Reference',
		collapsed: true,
		items: [
			{ label: 'Admin API', slug: 'reference/api' },
			{ label: 'Expressions and templates', slug: 'reference/expressions' },
			{ label: 'Connector definitions', slug: 'reference/connector-definitions' },
			{ label: 'Source mappings', slug: 'reference/mappings' },
		],
	},
	{
		label: 'Project',
		items: [
			{ label: 'Limits and what is not built', slug: 'limits' },
			{ label: 'License', slug: 'license' },
			{ label: 'FAQ', slug: 'faq' },
			{ label: 'Changelog', slug: 'changelog' },
			{ label: 'Verified claims', slug: 'verified' },
		],
	},
];
